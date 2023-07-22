use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use crate::{
    api::{Event, SevStep},
    types::kvm_page_track_mode,
};
use anyhow::{bail, Context, Result};
use log::{debug, info};

pub enum StateMachineNextAction {
    ///continue with next handler in chain
    NEXT,
    /// skip all remaining handlers
    SKIP,
    /// terminate
    SHUTDOWN,
}
pub trait EventHandler {
    fn process(&mut self, event: &Event, api: &mut SevStep) -> Result<StateMachineNextAction>;
    fn get_name(&self) -> &str;
}

pub struct SkipIfNotOnTargetGPAs {
    on_victim_pages: bool,
    target_gpas: HashSet<u64>,
    track_mode: kvm_page_track_mode,
    timer_value: u32,
    name: String,
}

impl SkipIfNotOnTargetGPAs {
    pub fn new(
        target_gpas: &[u64],
        track_mode: kvm_page_track_mode,
        timer_value: u32,
    ) -> SkipIfNotOnTargetGPAs {
        SkipIfNotOnTargetGPAs {
            on_victim_pages: false,
            target_gpas: HashSet::from_iter(target_gpas.iter().cloned()),
            track_mode,
            timer_value,
            name: "SkipIfNotOnTargetGPAs".to_string(),
        }
    }
}

impl EventHandler for SkipIfNotOnTargetGPAs {
    fn process(&mut self, event: &Event, api: &mut SevStep) -> Result<StateMachineNextAction> {
        let event = match event {
            Event::PageFaultEvent(v) => v,
            Event::StepEvent(_) => return Ok(StateMachineNextAction::NEXT),
        };

        if self.on_victim_pages {
            if self.target_gpas.contains(&event.faulted_gpa) {
                bail!("Internal state assumed to be on victim pages but got page fault for victim page. This should never happen");
            } else {
                debug!("Left victim pages with fault at GPA 0x{:x}. Disabling single stepping and re-tracking victim pages", event.faulted_gpa);

                api.stop_stepping()?;

                api.untrack_all_pages(self.track_mode)?;

                for x in &self.target_gpas {
                    api.track_page(*x, self.track_mode)
                        .with_context(|| format!("Failed to re-track target GPA 0x{:x}", x))?;
                }

                self.on_victim_pages = false;
            }
        } else {
            //not on victim pages
            if self.target_gpas.contains(&event.faulted_gpa) {
                debug!("Entering victim pages. Disabling single stepping and tracking all but the target GPAs");

                api.track_all_pages(self.track_mode)?;
                for x in &self.target_gpas {
                    api.untrack_page(*x, self.track_mode)
                        .with_context(|| format!("Failed to un-track GPA 0x:{:x}", x))?;
                }

                let mut gpas = self
                    .target_gpas
                    .iter()
                    .map(|x| x.clone())
                    .collect::<Vec<u64>>();
                api.start_stepping(self.timer_value, &mut gpas, true)?;

                self.on_victim_pages = true;
            } else {
                debug!(
                    "Not on victim pages and got page fault at 0x{:x} which is not on victim pages",
                    event.faulted_gpa
                );
            }
        }

        if self.on_victim_pages {
            Ok(StateMachineNextAction::NEXT)
        } else {
            Ok(StateMachineNextAction::SKIP)
        }
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}

pub struct BuildStepHistogram {
    step_histogram: HashMap<u64, u64>,
    event_counter: usize,
    name: String,
}

impl BuildStepHistogram {
    pub fn new() -> Self {
        BuildStepHistogram {
            step_histogram: HashMap::new(),
            event_counter: 0,
            name: "BuildStepHistogram".to_string(),
        }
    }
}

impl Display for BuildStepHistogram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.step_histogram)
    }
}

impl EventHandler for BuildStepHistogram {
    fn process(&mut self, event: &Event, _api: &mut SevStep) -> Result<StateMachineNextAction> {
        let event = match event {
            Event::PageFaultEvent(_) => return Ok(StateMachineNextAction::NEXT),
            Event::StepEvent(v) => v,
        };

        //update or create the counter for steps with size `e.retired_instructions`
        (*self
            .step_histogram
            .entry(event.retired_instructions as u64)
            .or_insert(0)) += 1;
        self.event_counter += 1;

        Ok(StateMachineNextAction::NEXT)
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}

pub struct StopAfterNStepsHandler {
    step_counter: usize,
    abort_thresh: usize,
    name: String,
}

impl StopAfterNStepsHandler {
    pub fn new(n: usize) -> StopAfterNStepsHandler {
        StopAfterNStepsHandler {
            step_counter: 0,
            abort_thresh: n,
            name: "StopAfterNStepsHandler".to_string(),
        }
    }
}

impl EventHandler for StopAfterNStepsHandler {
    fn process(&mut self, event: &Event, _api: &mut SevStep) -> Result<StateMachineNextAction> {
        let event = match event {
            Event::PageFaultEvent(_) => return Ok(StateMachineNextAction::NEXT),
            Event::StepEvent(v) => v,
        };

        debug!(
            "old step_counter={}, retired_instructions={}, abort_thresh={}",
            &self.step_counter, &event.retired_instructions, &self.abort_thresh
        );
        self.step_counter += event.retired_instructions as usize;

        if self.step_counter > self.abort_thresh {
            debug!("reached abort threshm");
            return Ok(StateMachineNextAction::SHUTDOWN);
        }

        return Ok(StateMachineNextAction::NEXT);
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}

pub struct TargetedStepper<'a, F>
where
    F: FnOnce() -> Result<()>,
    F: Send + 'static,
{
    api: SevStep<'a>,
    handler_chain: Vec<&'a mut dyn EventHandler>,
    track_mode: kvm_page_track_mode,
    initially_tracked_gpas: Vec<u64>,
    target_trigger: F,
}

impl<'a, F> TargetedStepper<'a, F>
where
    F: FnOnce() -> Result<()>,
    F: Send + 'static,
{
    pub fn new(
        api: SevStep<'a>,
        handler_chain: Vec<&'a mut dyn EventHandler>,
        initial_track_mode: kvm_page_track_mode,
        initially_tracked_gfns: Vec<u64>,
        target_trigger: F,
    ) -> TargetedStepper<'a, F> {
        TargetedStepper {
            api,
            handler_chain,
            track_mode: initial_track_mode,
            initially_tracked_gpas: initially_tracked_gfns,
            target_trigger,
        }
    }

    pub fn run(mut self) -> Result<()> {
        debug!("Performing initial tracking");
        for x in self.initially_tracked_gpas {
            self.api
                .track_page(x, self.track_mode)
                .context(format!("failed to track 0x{:x}", x))?;
            debug!("Tracking 0x{:x} with {:?}", x, self.track_mode);
        }

        info!("entering main event loop");

        //for the first event, trigger the target
        let mut event = self
            .api
            .block_untill_event(self.target_trigger)
            .context("error waiting for initial event")?;
        loop {
            for handler in &mut self.handler_chain {
                debug!("Running handler {}", handler.get_name());
                match handler.process(&event, &mut self.api)? {
                    StateMachineNextAction::NEXT => {
                        debug!("NEXT");
                    }
                    StateMachineNextAction::SKIP => {
                        debug!("SKIP");
                        self.api.ack_event();
                        break;
                    }
                    StateMachineNextAction::SHUTDOWN => {
                        debug!("SHUTDOWN");
                        self.api.ack_event();
                        info!("Left main event loop");
                        return Ok(());
                    }
                }
            }
            self.api.ack_event();

            //N.B. that we use an empty/NOP trigger now
            event = self
                .api
                .block_untill_event(|| Ok(()))
                .context("error waiting for event")?;
        }
    }
}
