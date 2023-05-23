use std::collections::{HashMap, HashSet};

use crate::{
    api::{Event, PageFaultEvent, SevStep, SevStepEvent},
    types::{kvm_page_track_mode, vmsa_register_name_t},
};
use anyhow::{bail, Context, Result};
use log::{debug, info};

enum StateMachineNextAction {
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

pub trait Middleware {
    fn process(&mut self, event :Event,api :&mut SevStep) -> Result<Box< dyn Middleware> >;
}

pub trait Finisher {
    fn build (&mut self, event: Event, api &mut SevStep) -> Result<StateMachineNextAction>;
}

/*
pub struct CheckVaddrsStateMachine {
    expected_vaddrs: Vec<u64>,
    next_vaddr: Box<dyn Iterator<Item = u64>>,
}

impl CheckVaddrsStateMachine {
    pub fn new(expected_vaddrs: Vec<u64>) -> CheckVaddrsStateMachine {
        CheckVaddrsStateMachine {
            expected_vaddrs,
            next_vaddr: Box::new(expected_vaddrs.iter()),
        }
    }
}*/

impl SteppingStateMachine for CheckVaddrsStateMachine {
    fn process(
        &mut self,
        event: &SevStepEvent,
        api: &mut SevStep,
    ) -> Result<StateMachineNextAction> {
        let rip = event
            .get_register(vmsa_register_name_t::VRN_RIP)
            .context("Failed to get RIP")?;

        match self.next_vaddr.next() {
            None => return Ok(StateMachineNextAction::FINISHED),
            Some(expected) => {
                if expected != rip {
                    bail!("Expected vaddr 0x{:x} got 0x{:x}", expected, rip);
                }
            }
        }
        Ok(StateMachineNextAction::CONTINUE)
    }
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

    fn handle_pf_event(&self, e: PageFaultEvent) -> Result<()> {
        if self.on_victim_pages {
            if self.target_gpas.contains(&e.faulted_gpa) {
                bail!("Internal state assumed to be on victim pages but got page fault for victim page. This should never happen");
            } else {
                debug!("Left victim pages with fault at GPA 0x{:x}. Disabling single stepping and re-tracking victim pages", e.faulted_gpa);

                self.api.stop_stepping()?;

                self.api.untrack_all_pages(self.track_mode)?;

                for x in self.target_gpas {
                    self.api
                        .track_page(x, self.track_mode)
                        .with_context(|| format!("Failed to re-track target GPA 0x{:x}", x))?;
                }

                self.on_victim_pages = false;
            }
        } else {
            //not on victim pages
            if self.target_gpas.contains(&e.faulted_gpa) {
                debug!("Entering victim pages. Disabling single stepping and tracking all but the target GPAs");

                self.api.track_all_pages(self.track_mode);
                for x in self.target_gpas {
                    self.api
                        .untrack_page(x, self.track_mode)
                        .with_context(|| format!("Failed to un-track GPA 0x:{:x}", x))?;
                }

                let gpas = self
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

        let mut on_victim_pages = false;
        loop {
            let event = self
                .api
                .block_untill_event()
                .context("error waiting for event")?;

            match event {
                Event::PageFaultEvent(v) => {
                    self.handle_pf_event(v)?;
                }
                Event::StepEvent(v) => match self.handle_step_event(v)? {
                    StateMachineNextAction::CONTINUE => {
                        debug!("State says CONTINUE");
                    }
                    StateMachineNextAction::FINISHED => {
                        debug!("State machine says FINISHED");
                        break;
                    }
                },
            }
        }
        info!("Left main event loop");
        Ok(())
    }
}
