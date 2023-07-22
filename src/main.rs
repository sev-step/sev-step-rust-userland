use std::sync::mpsc::channel;

use anyhow::{Ok, Result};
use log::debug;
use rust_userland::api::SevStep;
use rust_userland::single_stepper::{
    BuildStepHistogram, EventHandler, SkipIfNotOnTargetGPAs, StopAfterNStepsHandler,
    TargetedStepper,
};
use rust_userland::types::kvm_page_track_mode;
use rust_userland::vmserver_client::{self, SingleStepTarget};

fn main() -> Result<()> {
    env_logger::init();
    debug!("main running with debug logging!");
    let (tx, rx) = channel();

    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let mut _sev_step = SevStep::new(false, rx)?;

    let basepath = "http://localhost:8080";
    let victim_prog =
        vmserver_client::single_step_victim_init(basepath, SingleStepTarget::NopSlide)?;
    let timer_value = 0x38;

    let mut targetter = SkipIfNotOnTargetGPAs::new(
        &[victim_prog.gpa],
        kvm_page_track_mode::KVM_PAGE_TRACK_EXEC,
        timer_value,
    );
    let mut step_histogram = BuildStepHistogram::new();
    let mut stop_after = StopAfterNStepsHandler::new(100);
    let handler_chain: Vec<&mut dyn EventHandler> =
        vec![&mut targetter, &mut step_histogram, &mut stop_after];

    let stepper = TargetedStepper::new(
        _sev_step,
        handler_chain,
        kvm_page_track_mode::KVM_PAGE_TRACK_ACCESS,
        vec![victim_prog.gpa],
        move || vmserver_client::single_step_victim_start(basepath, SingleStepTarget::NopSlide),
    );

    stepper.run()?;

    println!("StepHistogram: {}", step_histogram);
    Ok(())
}
