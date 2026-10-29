use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use br_agentic_contract::{Completed, Opened, ResponseStream, ThreadError, ThreadRef, TurnTarget};
use br_llm_clients_core::{BoxFuture, StreamSink};
use br_llm_messages::{
    AgentId, EntryId, EntryView, ModelResponse, ResponseDelta, ResponseDraft, TurnEntry,
};
use br_runner_threads::ResponseSink;

use super::ToolkitThreadAccess;
use crate::core::OpenObject;
use crate::deltas;
use crate::error::{from_threads, refusal};

struct ToolkitResponseStream {
    sink: ResponseSink,
    alive: Arc<AtomicBool>,
    draft: Arc<Mutex<ResponseDraft>>,
}

impl StreamSink for ToolkitResponseStream {
    fn send<'a>(&'a mut self, delta: ResponseDelta) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.alive.load(Ordering::SeqCst) {
                return;
            }
            match self.sink.send(delta.clone()).await {
                Ok(()) => {
                    let mut draft = self.draft.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Err(error) = draft.apply(&delta) {
                        tracing::error!(%error, "a sent delta does not fit the draft of the response");
                    }
                }
                Err(error) => tracing::error!(%error, "a delta was not sent to threads"),
            }
        })
    }
}

impl ToolkitThreadAccess {
    pub(crate) async fn open_response_stream(
        &self,
        thread: &ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        entry: EntryId,
    ) -> Result<Opened<ResponseStream, ModelResponse>, ThreadError> {
        let who = self.who(agent)?;
        let projected = self.projected(thread, &who).await?;
        if let Ok(held) = projected.entry(&entry) {
            self.discard(agent, entry);
            return match held {
                EntryView::Turn(TurnEntry::ModelResponse(response)) => {
                    Ok(Opened::Held(response.clone()))
                }
                _ => Err(refusal("the thread holds the entry as another kind of entry").into()),
            };
        }
        Self::check_open_target(&projected, &who, &target)?;
        let open = self
            .stream_for(thread, &who, target.id(), &projected)
            .await?;
        let alive = Arc::new(AtomicBool::new(true));
        let draft = Arc::new(Mutex::new(ResponseDraft::new(target.id(), entry)));
        let previous = self
            .core
            .objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                entry,
                OpenObject {
                    agent,
                    alive: Arc::clone(&alive),
                    draft: Arc::clone(&draft),
                },
            );
        if let Some(previous) = previous {
            previous.alive.store(false, Ordering::SeqCst);
        }
        Ok(Opened::Stream(Box::new(ToolkitResponseStream {
            sink: open.handle.sink_factory().sink(),
            alive,
            draft,
        })))
    }

    pub(crate) async fn complete_model_response(
        &self,
        thread: &ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        response: ModelResponse,
    ) -> Result<Completed<ModelResponse>, ThreadError> {
        let who = self.who(agent)?;
        let projected = self.projected(thread, &who).await?;
        let entry_id = response.id();
        if let Ok(held) = projected.entry(&entry_id) {
            self.discard(agent, entry_id);
            return match held {
                EntryView::Turn(TurnEntry::ModelResponse(response)) => {
                    Ok(Completed::Held(response.clone()))
                }
                _ => Err(refusal("the thread holds the entry as another kind of entry").into()),
            };
        }
        Self::check_open_target(&projected, &who, &target)?;
        let TurnTarget::Ai { id, author } = &target else {
            return Err(refusal("a model response goes into an AI turn").into());
        };
        Self::check_entry(
            &projected,
            &who,
            *id,
            author,
            &TurnEntry::from(response.clone()),
        )?;
        let open = self.stream_for(thread, &who, *id, &projected).await?;
        let mut draft = self.sent_draft(agent, entry_id, *id);
        for delta in deltas::remaining(&draft, &response) {
            draft.apply(&delta).map_err(refusal)?;
            open.handle.delta(&delta).await.map_err(from_threads)?;
        }
        self.discard(agent, entry_id);
        Ok(Completed::Added)
    }

    fn sent_draft(
        &self,
        agent: AgentId,
        entry: EntryId,
        turn: br_llm_messages::TurnId,
    ) -> ResponseDraft {
        self.core
            .objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&entry)
            .filter(|open| open.agent == agent)
            .map(|open| {
                open.draft
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
            })
            .unwrap_or_else(|| ResponseDraft::new(turn, entry))
    }
}
