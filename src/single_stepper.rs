use std::collections::{HashMap, HashSet};

use crate::{
    api::{Event, PageFaultEvent, SevStep, SevStepEvent},
    types::{kvm_page_track_mode, vmsa_register_name_t},
};
use anyhow::{bail, Context, Result};
use log::{debug, info};

pub enum StateMachineNextAction {
    CONTINUE,
    FINISHED,
}
pub trait SteppingStateMachine {
    fn process(
        &mut self,
        event: &SevStepEvent,
        api: &mut SevStep,
    ) -> Result<StateMachineNextAction>;
}

pub struct TargetedStepper<'a, T>
where
    T: SteppingStateMachine,
{
    api: SevStep<'a>,
    target_gpas: HashSet<u64>,
    timer_value: u32,
    state_machine: T,
    track_mode: kvm_page_track_mode,

    ///if true, we are currently on one of the pages in `target_gpas``
    on_victim_pages: bool,
    /// maps step size to occurrence count
    step_histogram: HashMap<u64, u64>,
    step_counter: usize,
}

impl<'a, T> TargetedStepper<'a, T>
where
    T: SteppingStateMachine,
{
    pub fn new(
        api: SevStep,
        target_gpas: HashSet<u64>,
        timer_value: u32,
        state_machine: T,
        track_mode: kvm_page_track_mode,
    ) -> TargetedStepper<T> {
        TargetedStepper {
            api,
            target_gpas,
            timer_value,
            state_machine,
            track_mode,
            on_victim_pages: false,
            step_histogram: HashMap::new(),
            step_counter: 0,
        }
    }

    fn handle_pf_event(&mut self, e: PageFaultEvent) -> Result<()> {
        if self.on_victim_pages {
            if self.target_gpas.contains(&e.faulted_gpa) {
                bail!("Internal state assumed to be on victim pages but got page fault for victim page. This should never happen");
            } else {
                debug!("Left victim pages with fault at GPA 0x{:x}. Disabling single stepping and re-tracking victim pages", e.faulted_gpa);

                self.api.stop_stepping()?;

                self.api.untrack_all_pages(self.track_mode)?;

                for x in &self.target_gpas {
                    self.api
                        .track_page(*x, self.track_mode)
                        .with_context(|| format!("Failed to re-track target GPA 0x{:x}", x))?;
                }

                self.on_victim_pages = false;
            }
        } else {
            //not on victim pages
            if self.target_gpas.contains(&e.faulted_gpa) {
                debug!("Entering victim pages. Disabling single stepping and tracking all but the target GPAs");

                self.api.track_all_pages(self.track_mode);
                for x in &self.target_gpas {
                    self.api
                        .untrack_page(*x, self.track_mode)
                        .with_context(|| format!("Failed to un-track GPA 0x:{:x}", x))?;
                }

                let mut gpas = self
                    .target_gpas
                    .iter()
                    .map(|x| x.clone())
                    .collect::<Vec<u64>>();
                self.api.start_stepping(self.timer_value, &mut gpas, true)?;

                self.on_victim_pages = true;
            } else {
                debug!(
                    "Not on victim pages and got page fault at 0x{:x} which is not on victim pages",
                    e.faulted_gpa
                );
            }
        }
        Ok(())
    }

    pub fn handle_step_event(&mut self, e: SevStepEvent) -> Result<StateMachineNextAction> {
        if !self.on_victim_pages {
            bail!("handle_step_event got called but we are not on victim pages. This should never happen");
        }

        //update or create the counter for steps with size `e.retired_instructions`
        (*self
            .step_histogram
            .entry(e.retired_instructions as u64)
            .or_insert(0)) += 1;
        self.step_counter += 1;

        match e.retired_instructions {
            0 => {
                debug!("Total Steps {}, got zero step", self.step_counter);
                self.state_machine.process(&e, &mut self.api)
            }
            1 => {
                debug!(
                    "Total Steps {}, got single step, updating state machine",
                    self.step_counter
                );
                self.state_machine.process(&e, &mut self.api)
            }
            x => {
                bail!(
                    "Multi Step of size {} after {} steps",
                    e.retired_instructions,
                    self.step_counter
                );
            }
        }
    }

    pub fn run(&mut self) -> Result<()> {
        self.api
            .track_all_pages(self.track_mode)
            .context("initial track all failed")?;

        info!("entering main event loop");

        loop {
            let event = self
                .api
                .block_untill_event()
                .context("error waiting for event")?;

            match event {
                Event::PageFaultEvent(v) => {
                    self.handle_pf_event(v)?;
                }
                Event::StepEvent(v) => (), /*Event::StepEvent(v) => match self.handle_step_event(v)? {
                                               StateMachineNextAction::CONTINUE => {
                                                   debug!("State says CONTINUE");
                                               }
                                               StateMachineNextAction::FINISHED => {
                                                   debug!("State machine says FINISHED");
                                               }
                                           },*/
            }
        }
        info!("Left main event loop");
        Ok(())
    }
}
