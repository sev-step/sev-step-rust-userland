//!This program shows how to use the sev step framework to to exploit secret depending control flow leakge via single stepping

use anyhow::{Context, Result};
use clap::Parser;
use crossbeam::channel::bounded;
use iced_x86::code_asm::*;
use log::debug;
use sev_step_lib::{
    api::SevStep,
    config, vm_setup_helpers,
    vmserver_client::{self},
};
use vm_server::req_resp::InitAssemblyTargetReq;

/// Builds a dummy program that compares `guess``
/// to the "secret" constant 42. If the guess is correct, a branch with two NOPs and a RET is executed, oterhwise only a RET is executed. This is intended as an example, how single stepping can be used to leak a secret via counting the amount of executed instructions
fn build_cf_victim(guess: u64) -> Result<InitAssemblyTargetReq> {
    let mut a = CodeAssembler::new(64)?;

    let mut wrong_guess = a.create_label();
    a.mov(rax, 42_u64)?;
    a.mov(rsi, guess)?;
    a.cmp(rax, rsi)?;
    a.jne(wrong_guess)?;
    a.nop()?;
    a.nop()?;
    a.ret()?;
    a.set_label(&mut wrong_guess)?;
    a.ret()?;

    Ok(InitAssemblyTargetReq {
        code: a.take_instructions(),
        required_mem_bytes: 0,
    })
}
///This program demonstrates how to use the sev step API to infer secret dependent control flow.
/// It instructs the VM to execute a small program that branches on the value provided in the `guess_for_secret_input` argument. If the user inputs 42, the observed program will execute `7` instructions, otherwise `5`. In addition this program shows how to execute a callback function after the victim has executed a certain amount of steps.
#[derive(Parser, Debug)]
struct CliArgs {
    /// Path to vm config file
    #[arg(short, long, default_value = "./vm-config.toml")]
    vm_config_path: String,
    #[arg(short='t',long,value_parser=clap_num::maybe_hex::<u32>)]
    apic_timer_value: Option<u32>,
    #[arg(long)]
    guess_for_secret_input: u64,
}

fn main() -> Result<()> {
    env_logger::init();

    //parse args
    let args = CliArgs::parse();
    let vm_config =
        config::parse_config(&args.vm_config_path).context("failed to parse vm config")?;

    //To properly control the APIC timer, the victim VM must be pinned to a fixed core, that is isolated from the rest of the sytem. Isolating the core is described in this library's README.
    //In order to pin the VM to a CPU core, we use the QMP interface of QEMU, to look up the PID/TID of the process
    //that runs the VM's VCPU. Then we pin this process to a fixed core
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

    //In this example we use the "vmserver" that comes with SEV-Step. This component
    //is intended to quickly test attack ideas/scenarios. It allow us to first JIT assemble a
    //victim program using the `iced-x86` crate and subsequently load it to page aligned memory inside
    //the VM. In the response to the create reqeust, we get the GPA where our program is located.
    //After setting up some kind of tracking logic, we can request the execution of our program
    let victim = build_cf_victim(args.guess_for_secret_input)?;

    let _victim_program =
        vmserver_client::new_assembly_target(&vm_config.vm_server_address, &victim).context(
            format!("rquest to create assembly target {:?} failed", victim),
        )?;

    //Next we initialize the SEV-Step API. We pass in a channel that is hooked to CTR-C in order to terminate
    //the API connection at any time
    let (tx, abort_chan) = bounded(1);
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");
    let _sev_step = SevStep::new(false, abort_chan.clone(), false)?;

    //TODO: attack logic

    Ok(())
}
