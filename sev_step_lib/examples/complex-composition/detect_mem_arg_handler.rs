use enum_display::EnumDisplay;
use log::debug;
use std::collections::HashMap;

use sev_step_lib::{
    api::{Event, SevStep, SevStepError},
    event_handlers::{ComposableEventHandler, EventHandlerOutcome},
    single_stepper::StateMachineNextAction,
    types::kvm_page_track_mode,
};

#[derive(EnumDisplay)]
enum DetectMemArgHandlerState {
    //Victim is halted just before the exeuction of the target instruction
    BeforeTargetInstruction,
    //Targeted instruction is currently executing, gather page faults
    ExecutingTargetInstruction,
    //Targeted instruction has been executed
    AfterTargetInstruction,
}

pub struct DetectMemArgHandler {
    state: DetectMemArgHandlerState,
    //page faults encountered during the exeuction of the instruction
    recored_page_faults: Vec<u64>,
    single_step_time: u32,
}

impl DetectMemArgHandler {
    const NAME: &'static str = "DetectMemArgHandler";

    pub fn new(apic_timer: u32) -> DetectMemArgHandler {
        DetectMemArgHandler {
            state: DetectMemArgHandlerState::BeforeTargetInstruction,
            recored_page_faults: Vec::new(),
            single_step_time: apic_timer,
        }
    }

    pub fn get_observed_faults(&self) -> &Vec<u64> {
        &self.recored_page_faults
    }
}

impl ComposableEventHandler for DetectMemArgHandler {
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        _ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError> {
        let mut event = event.clone();
        debug!(
            "{}: called with event {:x?}",
            DetectMemArgHandler::NAME,
            event
        );
        loop {
            debug!("{}: at state {}", DetectMemArgHandler::NAME, self.state);
            match self.state {
                DetectMemArgHandlerState::BeforeTargetInstruction => {
                    api.track_all_pages(kvm_page_track_mode::KVM_PAGE_TRACK_WRITE)?;
                    api.start_stepping(self.single_step_time, &mut [], true)?;

                    self.state = DetectMemArgHandlerState::ExecutingTargetInstruction;
                }
                DetectMemArgHandlerState::ExecutingTargetInstruction => match &event {
                    Event::PageFaultEvent(v) => self.recored_page_faults.push(v.faulted_gpa),
                    Event::StepEvent(v) => match v.retired_instructions {
                        0 => (),
                        1 => {
                            debug!(
                                "{}: finished target instruction. Pending event is {:x?}",
                                DetectMemArgHandler::NAME,
                                event
                            );
                            self.state = DetectMemArgHandlerState::AfterTargetInstruction;
                            api.stop_stepping()?;
                            api.untrack_all_pages(kvm_page_track_mode::KVM_PAGE_TRACK_WRITE)?;

                            //N.B. that the event is not yet acked at this point (as requested by our "contract")
                            return Ok(EventHandlerOutcome {
                                pending_event: event,
                                next_action: StateMachineNextAction::NEXT,
                            });
                        }
                        _ => return Err(SevStepError::MultiStep { event: v.clone() }),
                    },
                },
                DetectMemArgHandlerState::AfterTargetInstruction => todo!(),
            }
            api.ack_event();
            event = api.block_untill_event(|| Ok(()), None)?;
        }
    }

    fn get_name(&self) -> &str {
        DetectMemArgHandler::NAME
    }
}
