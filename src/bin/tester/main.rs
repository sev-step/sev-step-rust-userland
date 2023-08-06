use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use crossbeam::channel::bounded;
use log::debug;
use rust_userland::{api::SevStep, config, vm_setup_helpers};
use test::TestGroup;

use crate::test::{Test, TestName};

pub mod test;

#[derive(Parser, Debug)]
struct CliArgs {
    /// Path to vm config file
    #[arg(short, long, default_value = "./vm-config.toml")]
    vm_config_path: String,
    /// Run the given group of tests
    #[arg(long, group = "test_mode")]
    test_group: Option<TestGroup>,
    /// Run the listed, individual tests
    #[arg(long, group = "test_mode")]
    tests: Option<Vec<TestName>>,
}

fn main() -> Result<()> {
    env_logger::init();

    //parse args
    let args = CliArgs::parse();
    let vm_config =
        config::parse_config(&args.vm_config_path).context("failed to parse vm config")?;

    //cpu pinning for VM and ourself
    debug!("main running with debug logging!");
    let vcpu_thread_id = vm_setup_helpers::get_vcpu_thread_id(&vm_config.qemu_qmp_address)
        .context("failed to get VCPU thread id")?;
    debug!("vcpu_thread_id is {}", vcpu_thread_id);

    vm_setup_helpers::pin_pid_to_cpu(vcpu_thread_id, vm_config.vm_cpu_core).context(format!(
        "failed to pin vcpu (tid {}) to core {}",
        vcpu_thread_id, vm_config.vm_cpu_core,
    ))?;
    debug!(
        "Pinned vcpu_thread (tid {}) to core {}",
        vcpu_thread_id, vm_config.vm_cpu_core
    );

    //instantiate tests
    let mut selected_tests = Vec::new();
    if let Some(v) = args.test_group {
        selected_tests.append(&mut v.into())
    } else if let Some(v) = args.tests {
        for t in v {
            selected_tests.push(t)
        }
    } else {
        panic!("Error in CLI parsing logic")
    }
    debug!("selected_tests: {:?}", selected_tests);

    //mapping ctrl-c to channel
    let (tx, rx) = bounded(1);
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let tests: Vec<Box<dyn Test>> = selected_tests
        .iter()
        .map(|t| t.instantiate(rx.clone(), vm_config.vm_server_address.clone()))
        .collect::<Result<_>>()
        .context(format!(
            "failed to instantiate at least one of the selected tests {:?}",
            selected_tests
        ))?;

    //runs tests
    let mut successful_tests = 0;
    let test_count = tests.len();
    for (idx, t) in tests.into_iter().enumerate() {
        println!(
            "Running test [{}/{}]: {}",
            idx + 1,
            test_count,
            t.get_name()
        );
        match t
            .run()
            .context(format!("Test {} {}", t.get_name(), "failed".red()))
        {
            Ok(_) => {
                successful_tests += 1;
                println!("{}", "SUCCESS".green());
            }
            Err(e) => println!("{} with {}", "FAILED".red(), e),
        }
    }
    if successful_tests == test_count {
        println!("{}", "All tests succeeded".green());
    } else {
        println!(
            "{}, {} out of {} tests succeeded",
            "ONLY".yellow(),
            successful_tests,
            test_count
        );
    }

    Ok(())
}
