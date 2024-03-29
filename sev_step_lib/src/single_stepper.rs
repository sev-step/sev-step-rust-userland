//!
//! This file contains abstractions that automate common use cases of the library.
//! The different abstractions, called `EventHandlers` are similar to HTTP middleware, in the sense
//! that they can be chained together, to achieve more complex behavior
//! The [`TargetedStepper`](struct@TargetedStepper) can be used to "execute" a chain of event handlers.
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    time::Duration,
};

use crate::{
    api::{Event, SevStep, SevStepError},
    types::*,
};
use anyhow::{anyhow, bail, Context, Result};
use log::{debug, error, info};

pub enum StateMachineNextAction {
    ///continue with next handler in chain
    NEXT,
    /// skip all remaining handlers
    SKIP,
    /// terminate
    SHUTDOWN,
    /// terminated due to an error, attached string describes reason
    ErrorShutdown(String),
}
pub trait EventHandler {
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<StateMachineNextAction>;
    fn get_name(&self) -> &str;
}

/// Tracks a set of GPAs with the given track mode.
/// All GPAs are automatically re-tracked upon subsequent page fault events
/// Does NOT break track loops where no progress is made inside VM
/// Assumes pages are initially tracked
pub struct RetrackGPASet {
    gpas: HashSet<u64>,
    name: String,
    track_mode: kvm_page_track_mode,
    gpa_for_retrack: Option<u64>,
    iteration_count: usize,
    max_iterations: Option<usize>,
}

impl RetrackGPASet {
    const CK_CURRENT_GPA: &'static str = "RetrackGPASet_Current_GPA";

    /// # Arguments
    /// * `gpas` gpas that should be tracked
    /// * `track_mode` selects the used tracking mode
    /// * `max_iterations` if set, terminate this handler after the given iteration count by returning [`StateMachineNextAction::SHUTDOWN`] in [`Self::process()`]
    pub fn new(
        gpas: HashSet<u64>,
        track_mode: kvm_page_track_mode,
        max_iterations: Option<usize>,
    ) -> Self {
        RetrackGPASet {
            gpas,
            track_mode,
            name: "RetrackGPASet".to_string(),
            gpa_for_retrack: None,
            iteration_count: 0,
            max_iterations,
        }
    }

    ///Retrieve the GPA of the last pagefault event processed by this handler
    pub fn get_current_gpa_from_ctx(ctx: &HashMap<String, Vec<u8>>) -> Result<u64> {
        let serialized_data = match ctx.get(Self::CK_CURRENT_GPA) {
            Some(v) => v,
            None => bail!("data not present"),
        };
        let deserialized_data: Result<u64, _> = bincode::deserialize(&serialized_data);
        match deserialized_data {
            Ok(v) => return Ok(v),
            Err(e) => bail!(format!("failed to deserialize : {:?}", e)),
        };
    }

    fn update_current_gpa_in_ctx(gpa: u64, ctx: &mut HashMap<String, Vec<u8>>) -> Result<()> {
        let serialized_data = bincode::serialize(&gpa)?;
        ctx.insert(String::from(Self::CK_CURRENT_GPA), serialized_data);
        Ok(())
    }
}

impl EventHandler for RetrackGPASet {
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<StateMachineNextAction> {
        match &event {
            Event::PageFaultEvent(pf_event) => {
                Self::update_current_gpa_in_ctx(pf_event.faulted_gpa, ctx)?;

                if let Some(gpa) = self.gpa_for_retrack {
                    if gpa == pf_event.faulted_gpa {
                        bail!("got second fault for gpa 0x{:x}", gpa);
                    }
                    api.track_page(gpa, self.track_mode)
                        .context(format!("failed to re-track gpa 0x{:x}", gpa))?;
                    self.gpa_for_retrack = None;
                }

                if self.gpas.contains(&pf_event.faulted_gpa) {
                    self.gpa_for_retrack = Some(pf_event.faulted_gpa);
                }

                self.iteration_count += 1;
                if let Some(max_iterations) = self.max_iterations {
                    if self.iteration_count >= max_iterations {
                        return Ok(StateMachineNextAction::SHUTDOWN);
                    }
                }
                Ok(StateMachineNextAction::NEXT)
            }
            Event::StepEvent(_) => Ok(StateMachineNextAction::NEXT),
        }
    }

    fn get_name(&self) -> &str {
        &self.name
    }
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
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        _ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<StateMachineNextAction> {
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

                let mut gpas = self.target_gpas.iter().copied().collect::<Vec<u64>>();
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
    ///Returns HapMap, that maps encountered step sizes to their occurrence count
    pub fn get_values(&self) -> &HashMap<u64, u64> {
        &self.step_histogram
    }
}

impl Display for BuildStepHistogram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.step_histogram)
    }
}

impl EventHandler for BuildStepHistogram {
    fn process(
        &mut self,
        event: &Event,
        _api: &mut SevStep,
        _ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<StateMachineNextAction> {
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

pub struct SimpleCallbackAfterNSingleStepsHandler<T, F>
where
    T: Fn(&usize) -> bool,
    F: FnMut(&mut SevStep, &Event) -> Result<()>,
{
    //total step count
    step_counter: usize,
    ///maps step counts to the function that should be called at this step count
    callbacks: Vec<(T, F)>,
    name: String,
}

impl<T, F> SimpleCallbackAfterNSingleStepsHandler<T, F>
where
    T: Fn(&usize) -> bool,
    F: FnMut(&mut SevStep, &Event) -> Result<()>,
{
    pub fn new(callbacks: Vec<(T, F)>) -> SimpleCallbackAfterNSingleStepsHandler<T, F> {
        SimpleCallbackAfterNSingleStepsHandler {
            step_counter: 0,
            callbacks,
            name: "SimpleCallbackAfterNSingleStepsHandler".to_string(),
        }
    }
}

impl<T, F> EventHandler for SimpleCallbackAfterNSingleStepsHandler<T, F>
where
    T: Fn(&usize) -> bool,
    F: FnMut(&mut SevStep, &Event) -> Result<()>,
{
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        _ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<StateMachineNextAction> {
        //check and execute callbacks
        for (idx, (should_exec, callback)) in self.callbacks.iter_mut().enumerate() {
            let should_exec_result = should_exec(&self.step_counter);
            debug!(
                "step_count={}, callback idx={}, should_exec returned {} ",
                self.step_counter, idx, should_exec_result
            );
            if should_exec_result {
                callback(api, event)
                    .context(format!("error executing callback with index {idx}",))?;
            }
        }
        self.step_counter += 1;

        Ok(StateMachineNextAction::NEXT)
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}

pub struct StopAfterNSingleStepsHandler {
    step_counter: usize,
    abort_thresh: usize,
    name: String,
    expected_rip_values: Option<Vec<u64>>,
}

impl StopAfterNSingleStepsHandler {
    const CK_STEPS: &'static str = "CK_StopAfterNSingleStepsHandler_Step_Counter";

    ///Counts the number of single step events (i.e. only step size 1). Aborts execution after a given amount of steps.
    /// # Arguments
    /// * `n` : Return [`StateMachineNextAction::SHUTDOWN`] in [`Self::process()`] after this many single step events (zero steps are discarded)
    /// * `expected_rip_values` : If set, at each step compare RIP against the given value. Requires VM to run
    /// in debug mode. May be less than `n`
    pub fn new(n: usize, expected_rip_values: Option<Vec<u64>>) -> StopAfterNSingleStepsHandler {
        StopAfterNSingleStepsHandler {
            step_counter: 0,
            abort_thresh: n,
            name: "StopAfterNStepsHandler".to_string(),
            expected_rip_values,
        }
    }

    pub fn get_step_counter_from_ctx(ctx: &HashMap<String, Vec<u8>>) -> Result<usize> {
        let serialized_data = match ctx.get(Self::CK_STEPS) {
            Some(v) => v,
            None => bail!("data not present"),
        };
        let deserialized_data: Result<usize, _> = bincode::deserialize(&serialized_data);
        match deserialized_data {
            Ok(v) => return Ok(v),
            Err(e) => bail!(format!("failed to deserialize : {:?}", e)),
        };
    }

    fn update_step_counter_in_ctx(
        step_counter: usize,
        ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<()> {
        let serialized_data = bincode::serialize(&step_counter)?;
        ctx.insert(String::from(Self::CK_STEPS), serialized_data);
        Ok(())
    }
}

impl EventHandler for StopAfterNSingleStepsHandler {
    fn process(
        &mut self,
        event: &Event,
        _api: &mut SevStep,
        ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<StateMachineNextAction> {
        let event = match event {
            Event::PageFaultEvent(_) => return Ok(StateMachineNextAction::NEXT),
            Event::StepEvent(v) => v,
        };

        debug!(
            "old step_counter={}, retired_instructions={}, abort_thresh={}",
            &self.step_counter, &event.retired_instructions, &self.abort_thresh
        );

        if event.retired_instructions == 0 {
            return Ok(StateMachineNextAction::NEXT);
        }

        if let Some(exepcted_rip_values) = &self.expected_rip_values {
            if exepcted_rip_values.len() > self.step_counter {
                let got_rip = event
                    .get_register(vmsa_register_name_t::VRN_RIP)
                    .ok_or(anyhow!(
                        "failed to get RIP to compare against expected rip values"
                    ))?;
                let want_rip = exepcted_rip_values[self.step_counter];
                if want_rip != got_rip {
                    bail!(
                        "at step {}, expected RIP 0x{:x} got 0x{:x}",
                        self.step_counter + 1,
                        want_rip,
                        got_rip,
                    );
                }
            }
        }

        self.step_counter += 1;
        Self::update_step_counter_in_ctx(self.step_counter, ctx)
            .context("update_step_counter_in_ctx failed")?;

        if self.step_counter > self.abort_thresh {
            debug!("reached abort thresh");
            return Ok(StateMachineNextAction::SHUTDOWN);
        }

        Ok(StateMachineNextAction::NEXT)
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
    timeout: Option<Duration>,
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
        initially_tracked_gpas: Vec<u64>,
        target_trigger: F,
        timeout: Option<Duration>,
    ) -> TargetedStepper<'a, F> {
        TargetedStepper {
            api,
            handler_chain,
            track_mode: initial_track_mode,
            initially_tracked_gpas,
            target_trigger,
            timeout,
        }
    }

    pub fn run(mut self) -> Result<(), SevStepError> {
        debug!("Performing initial tracking");
        for x in self.initially_tracked_gpas {
            self.api
                .track_page(x, self.track_mode)
                .context(format!("failed to track 0x{:x}", x))?;
            debug!("Tracking 0x{:x} with {:?}", x, self.track_mode);
        }

        let mut ctx = HashMap::new();
        info!("entering main event loop");

        //for the first event, trigger the target
        let mut event = self
            .api
            .block_untill_event(self.target_trigger, self.timeout)?;
        loop {
            debug!("Got Event {:X?}", event);
            for handler in &mut self.handler_chain {
                debug!("Running handler {}", handler.get_name());
                match handler.process(&event, &mut self.api, &mut ctx)? {
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
                    StateMachineNextAction::ErrorShutdown(message) => {
                        error!("ERROR_SHUTDOWN with message={}", message);
                        return Err(anyhow!(
                            "logic error in handler {} : {}",
                            handler.get_name(),
                            message
                        )
                        .into());
                    }
                }
            }
            self.api.ack_event();

            //N.B. that we use an empty/NOP trigger now
            event = self.api.block_untill_event(|| Ok(()), self.timeout)?;
        }
    }
}
