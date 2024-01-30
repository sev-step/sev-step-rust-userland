use std::{collections::HashMap, time::Duration};

use log::{debug, error, info};

use crate::{
    api::{Event, SevStep, SevStepError},
    single_stepper::StateMachineNextAction,
    types::kvm_page_track_mode,
};
use anyhow::anyhow;

pub mod closure_adapter_handler;
pub mod state_machine_handlers;

pub struct EventHandlerOutcome {
    //Event handler are reuired to return an event, i.e. ensure that the victim is in a paused state. If the victim does not ack an event, it should return the event it was called with
    pub pending_event: Event,
    // indicates to the executor how to proceed (if the event handler is part of a chain)
    pub next_action: StateMachineNextAction,
}

pub trait ComposableEventHandler {
    ///#Arguments
    /// - `event` most recent event. A handler might ack the event
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError>;
    fn get_name(&self) -> &str;
}

pub struct ComposableHandlerChain<'a, F>
where
    F: FnOnce() -> Result<(), anyhow::Error>,
    F: Send + 'static,
{
    api: SevStep<'a>,
    handler_chain: Vec<&'a mut dyn ComposableEventHandler>,
    initial_tracking: Option<InitialTrackingRequest>,
    target_trigger: Option<F>,
    timeout: Option<Duration>,
}

pub struct ComposableHandlerChainOutcome {
    pub pending_event: Event,
    pub produced_ctx: HashMap<String, Vec<u8>>,
}

pub struct InitialTrackingRequest {
    pub mode: kvm_page_track_mode,
    pub gpas: Vec<u64>,
}

impl<'a, F> ComposableHandlerChain<'a, F>
where
    F: FnOnce() -> Result<(), anyhow::Error>,
    F: Send + 'static,
{
    pub fn new(
        api: SevStep<'a>,
        handler_chain: Vec<&'a mut dyn ComposableEventHandler>,
        initial_tracking: Option<InitialTrackingRequest>,
        target_trigger: Option<F>,
        timeout: Option<Duration>,
    ) -> ComposableHandlerChain<'a, F> {
        ComposableHandlerChain {
            api,
            handler_chain,
            initial_tracking,
            target_trigger,
            timeout,
        }
    }

    pub fn run(mut self) -> Result<ComposableHandlerChainOutcome, SevStepError> {
        debug!("Performing initial tracking");
        if let Some(initial_tracking) = self.initial_tracking {
            for x in initial_tracking.gpas {
                self.api.track_page(x, initial_tracking.mode)?;
                debug!("Tracking 0x{:x} with {:?}", x, initial_tracking.mode);
            }
        }

        let mut ctx = HashMap::new();
        info!("entering main event loop");

        //For the first event, we might need to execute target_trigger
        let mut event = match self.target_trigger {
            None => self.api.block_untill_event(|| Ok(()), self.timeout),
            Some(trigger) => self.api.block_untill_event(trigger, self.timeout),
        }?;

        debug!("Got Event {:X?}", event);
        let handler_count = self.handler_chain.len();
        for (handler_idx, handler) in self.handler_chain.iter_mut().enumerate() {
            info!(
                "Running handler {} [{}/{}]",
                handler.get_name(),
                handler_idx,
                handler_count,
            );
            let handler_outcome = handler.process(&event, &mut self.api, &mut ctx)?;
            event = handler_outcome.pending_event;

            match handler_outcome.next_action {
                StateMachineNextAction::NEXT => {
                    debug!("NEXT");
                }
                StateMachineNextAction::SKIP => {
                    panic!("todo: composeable handler chain does not support StateMachineNextAction::SKIP");
                }
                StateMachineNextAction::SHUTDOWN => {
                    debug!("SHUTDOWN");
                    //N.B. that we keep the event pending. This allows the caller
                    //to e.g. execute another handler chain
                    return Ok(ComposableHandlerChainOutcome {
                        pending_event: event,
                        produced_ctx: ctx,
                    });
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
            };
        }

        Ok(ComposableHandlerChainOutcome {
            pending_event: event,
            produced_ctx: ctx,
        })
    }
}
