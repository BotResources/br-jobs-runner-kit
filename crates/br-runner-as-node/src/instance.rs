use std::sync::Arc;

use br_graph_contract::{CancelView, Event, EventSender, Fields, InputReader, InstanceId, State};
use br_runner_jobs::{CallMode, CallerHandle, CreateChildInput, PayloadForm, WriteEnd};
use uuid::Uuid;

use crate::error::Error;
use crate::finish;
use crate::node::Parts;
use crate::signals::{Signals, Stop};
use crate::spec::{JobSpec, Payload};
use crate::sync_call;

pub(crate) async fn run<F: Fields>(
    parts: Arc<Parts<F>>,
    max_attempts: Option<i64>,
    id: InstanceId,
    mut reader: InputReader<F>,
    sender: EventSender<F>,
    cancel: CancelView,
) {
    let mut signals = Signals::new(&cancel, &sender);
    let ended = step(&parts, max_attempts, id, &mut reader, &cancel, &mut signals).await;
    match ended {
        Ok(event) | Err(Stop::Event(event)) => settle(&mut reader, sender, event),
        Err(Stop::Gone) => {}
    }
}

fn settle<F: Fields>(reader: &mut InputReader<F>, sender: EventSender<F>, event: Event<F>) {
    reader.close();
    drop(reader.drain());
    sender.finish(event);
}

async fn last_state<F: Fields>(
    reader: &mut InputReader<F>,
    signals: &mut Signals,
) -> Result<Arc<State<F>>, Stop<F>> {
    let first = signals.guard(reader.next()).await?.ok_or(Stop::Gone)?;
    reader.close();
    let later = reader.drain();
    Ok(later
        .into_iter()
        .last()
        .map_or_else(|| first.into_state(), |input| input.into_state()))
}

async fn step<F: Fields>(
    parts: &Parts<F>,
    max_attempts: Option<i64>,
    id: InstanceId,
    reader: &mut InputReader<F>,
    cancel: &CancelView,
    signals: &mut Signals,
) -> Result<Event<F>, Stop<F>> {
    let state = last_state(reader, signals).await?;
    let spec = signals
        .guard((parts.request)(Arc::clone(&state)))
        .await?
        .map_err(Stop::failed)?;
    let caller = signals
        .guard(parts.caller_source.caller(id))
        .await?
        .map_err(|cause| Stop::failed(Error::CallerSource(cause)))?;
    let child = Uuid::now_v7();
    let end_wait = caller.end_wait(child);
    let create_end = hand_over(&caller, parts, max_attempts, child, spec)?;
    let outcome = sync_call::wait(&caller, child, end_wait, create_end, signals).await?;
    finish::hand_over_unless_cancelled(cancel, &caller, child, &outcome)?;
    let update = signals
        .guard((parts.response)(state, outcome))
        .await?
        .map_err(Stop::failed)?;
    Ok(Event::Result(update))
}

fn hand_over<F: Fields>(
    caller: &CallerHandle,
    parts: &Parts<F>,
    max_attempts: Option<i64>,
    child: Uuid,
    spec: JobSpec,
) -> Result<WriteEnd, Stop<F>> {
    let payload = match spec.payload {
        Payload::Inline(content) => PayloadForm::Inline(content),
        Payload::Kept(content) => {
            caller
                .upload_payload(child, content)
                .map_err(|error| Stop::failed(Error::Upload(Box::new(error))))?;
            PayloadForm::Kept
        }
        Payload::Reference {
            service,
            document_id,
        } => PayloadForm::Reference {
            service,
            document_id,
        },
    };
    let mut input = CreateChildInput::new(
        child,
        Uuid::now_v7(),
        parts.runner_type.as_str(),
        CallMode::Sync,
        payload,
    );
    input.config = spec.config;
    input.max_attempts = max_attempts;
    caller
        .create_child(input)
        .map_err(|error| Stop::failed(Error::Create(Box::new(error))))
}
