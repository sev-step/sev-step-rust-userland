use std::collections::HashMap;

use anyhow::anyhow;
use log::{debug, warn};

use crate::{
    api::{Event, SevStep, SevStepError},
    single_stepper::StateMachineNextAction,
    types::vmsa_register_name_t,
};

use super::{ComposableEventHandler, EventHandlerOutcome};

pub enum SequenceMatchingStrategy {
    ///If expected page fault sequence is interrupted by other page faults, rest progresss to start of sequence
    StrictWithReset,
    ///If expected page fault sequence is interrupted by other page faults, abort with [`StateMachineNextAction::ERROR_SHUTDOWN`]
    StrictWithAbort,
    ///Expected page fault sequence may be interrupted by other page faults
    Scattered,
}
pub struct SkipUntilPageFaultSequence {
    name: String,
    idx_next_pf: usize,
    pf_sequence: Vec<u64>,
    matching: SequenceMatchingStrategy,
}

impl SkipUntilPageFaultSequence {
    /// Handler that expects page faults events and consumes them until the requested sequence has been encountered.
    /// Expects that tracking is already configured.
    /// # Arguments
    /// - `pf_sequence`: sequence of page faults that we want to observe before returning
    /// - `matching`: configures if it is ok for `pf_sequence` to be interrupted by faults at other addresses
    pub fn new(
        pf_sequence: Vec<u64>,
        matching: SequenceMatchingStrategy,
    ) -> SkipUntilPageFaultSequence {
        SkipUntilPageFaultSequence {
            name: "SkipUntilPageFaultSequence".to_string(),
            idx_next_pf: 0,
            pf_sequence,
            matching,
        }
    }
}

impl ComposableEventHandler for SkipUntilPageFaultSequence {
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        _ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError> {
        let mut event = event.clone();
        let mut first_iteration = true;
        loop {
            if !first_iteration {
                api.ack_event();
                debug!("SkipUntilPageFaultSequence:  waiting for next event...");
                event = api.block_untill_event(|| Ok(()), None)?;
                debug!("SkipUntilPageFaultSequence: Got event");
            } else {
                debug!("SkipUntilPageFaultSequence: first iteration, not waiting for event");
                first_iteration = false;
            }

            let pf_event = match &event {
                Event::PageFaultEvent(v) => v,
                Event::StepEvent(v) => {
                    warn!("SkipUntilPageFaultSequence encountered {:?}", v);
                    continue;
                }
            };

            debug!("SkipUntilPageFaultSequence: got {:x?}", &pf_event);
            let expected_gpa = self.pf_sequence[self.idx_next_pf];
            if pf_event.faulted_gpa == expected_gpa {
                debug!("SkipUntilPageFaultSequence: Got expected fault");
                self.idx_next_pf += 1;
            } else {
                debug!("SkipUntilPageFaultSequence: unexpected fault");
                match self.matching {
                    SequenceMatchingStrategy::StrictWithReset => {
                        self.idx_next_pf = 0;
                        debug!("SkipUntilPageFaultSequence: resetting progress");
                    }
                    SequenceMatchingStrategy::StrictWithAbort => {
                        debug!("SkipUntilPageFaultSequence: aborting");
                        return Ok(EventHandlerOutcome {
                        pending_event: event.clone(),
                        next_action: StateMachineNextAction::ErrorShutdown(format!("requested StrictWithAbort matching and at idx {} we got fault at gpa 0x{:} instead of expected 0x{:x}",self.idx_next_pf,pf_event.faulted_gpa,expected_gpa)),
                    });
                    }
                    SequenceMatchingStrategy::Scattered => {
                        debug!("SkipUntilPageFaultSequence: doing scattered matching, unexpected is fine")
                    }
                }
            }

            if self.idx_next_pf == self.pf_sequence.len() {
                return Ok(EventHandlerOutcome {
                    pending_event: event.clone(),
                    next_action: StateMachineNextAction::NEXT,
                });
            }
        }
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}

pub struct SkipUntilNSingleSteps {
    step_counter: usize,
    wanted_step_count: usize,
    expected_rip_values: Option<Vec<u64>>,
    //counts number of consecutive zero steps. Used to implement abort logic
    consecutive_zero_steps: usize,
}

impl SkipUntilNSingleSteps {
    const NAME: &'static str = "SkipUntilNSingleSteps";
    //used in conjunction with the `consecutive_zero_steps` member, to abort zero stepping "loops"
    const ZERO_STEP_ABORT_THRESH: usize = 10;
    ///Handler that consumes the next [``] single step events. Page fault events are ignored.
    /// Expects single stepping to be configured. Does not disable single stepping
    /// # Arguments
    /// - `wanted_step_count`: number of single steps that should be consumed
    /// - `expected_rip_values`: If Some AND debug mode enabled return [`StateMachineNextAction::ERROR_SHUTDOWN`] if the single steps dont have the given rip values.
    pub fn new(
        wanted_step_count: usize,
        expected_rip_values: Option<Vec<u64>>,
    ) -> SkipUntilNSingleSteps {
        SkipUntilNSingleSteps {
            step_counter: 0,
            wanted_step_count,
            expected_rip_values,
            consecutive_zero_steps: 0,
        }
    }
}

impl ComposableEventHandler for SkipUntilNSingleSteps {
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        _ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError> {
        let mut event = event.clone();
        let mut first_iteration = true;
        debug!("SkipUntilNSingleSteps: invoked on event {:x?}", event);
        loop {
            if !first_iteration {
                api.ack_event();
                event = api.block_untill_event(|| Ok(()), None)?;
            } else {
                first_iteration = false;
            }

            let step_event = match &event {
                Event::PageFaultEvent(v) => {
                    debug!("SkipUntilNSingleSteps: got page fault event {:x?}", v);
                    continue;
                }
                Event::StepEvent(v) => v,
            };

            if step_event.retired_instructions == 0 {
                self.consecutive_zero_steps += 1;
                if self.consecutive_zero_steps > SkipUntilNSingleSteps::ZERO_STEP_ABORT_THRESH {
                    return Ok(EventHandlerOutcome {
                        pending_event: event.clone(),
                        next_action: StateMachineNextAction::ErrorShutdown(format!(
                            "SkipUntilNSingleSteps: got {} consecutive zero steps",
                            self.consecutive_zero_steps
                        )),
                    });
                }
                debug!(
                    "{}: got zero step {:x?}",
                    SkipUntilNSingleSteps::NAME,
                    step_event
                );
                continue;
            } else {
                self.consecutive_zero_steps = 0;
            }

            if let Some(expected_rip_values) = &self.expected_rip_values {
                if expected_rip_values.len() > self.step_counter {
                    let got_rip = step_event
                        .get_register(vmsa_register_name_t::VRN_RIP)
                        .ok_or(anyhow!(
                            "failed to get RIP to compare against expected rip values"
                        ))?;
                    let want_rip = expected_rip_values[self.step_counter];
                    if want_rip != got_rip {
                        return Ok(EventHandlerOutcome {
                            pending_event: event.clone(),
                            next_action: StateMachineNextAction::ErrorShutdown(format!(
                                "at step {}, expected RIP 0x{:x} got 0x{:x}",
                                self.step_counter + 1,
                                want_rip,
                                got_rip,
                            )),
                        });
                    }
                }
            }

            self.step_counter += 1;

            if self.step_counter == self.wanted_step_count {
                return Ok(EventHandlerOutcome {
                    pending_event: event.clone(),
                    next_action: StateMachineNextAction::NEXT,
                });
            }
        }
    }

    fn get_name(&self) -> &str {
        SkipUntilNSingleSteps::NAME
    }
}
