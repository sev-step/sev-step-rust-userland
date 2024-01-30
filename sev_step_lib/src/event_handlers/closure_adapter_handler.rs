///! Allows to use a closure as a ComposeableEventHandler. This is useful to
/// glue pre-built handlers together with custom logic
use std::collections::HashMap;

use crate::api::{Event, SevStep, SevStepError};

use super::{ComposableEventHandler, EventHandlerOutcome};

/// Allows to use a closure as a ComposeableEventHandler. This is useful to
/// glue pre-built handlers together with custom logic
pub struct ClosureAdapterHandler<F>
where
    F: FnMut(
        &Event,
        &mut SevStep,
        &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError>,
{
    name: String,
    payload: F,
}

impl<F> ClosureAdapterHandler<F>
where
    F: FnMut(
        &Event,
        &mut SevStep,
        &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError>,
{
    pub fn new(description: &str, payload: F) -> Self {
        Self {
            name: description.to_string(),
            payload,
        }
    }
}

impl<F> ComposableEventHandler for ClosureAdapterHandler<F>
where
    F: FnMut(
        &Event,
        &mut SevStep,
        &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError>,
{
    fn process(
        &mut self,
        event: &Event,
        api: &mut SevStep,
        ctx: &mut HashMap<String, Vec<u8>>,
    ) -> Result<EventHandlerOutcome, SevStepError> {
        (self.payload)(&event, api, ctx)
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}
