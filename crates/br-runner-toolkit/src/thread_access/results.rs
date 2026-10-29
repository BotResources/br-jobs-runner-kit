use br_agentic_contract::{
    Completed, Opened, ResultStream, ResultStreamBox, ThreadError, ThreadRef, TurnTarget,
};
use br_llm_clients_core::BoxFuture;
use br_llm_messages::{
    AgentId, BlockId, EntryId, EntryView, RuntimeToolResult, RuntimeToolResults, Turn, TurnEntry,
};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicBool;

use super::ToolkitThreadAccess;
use crate::core::OpenObject;
use crate::error::{from_threads, refusal};

struct DiscardingResultStream;

impl ResultStream for DiscardingResultStream {
    fn send<'a>(&'a mut self, _fragment: String) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

impl ToolkitThreadAccess {
    pub(crate) async fn open_result_stream(
        &self,
        thread: &ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        entry: EntryId,
        call: BlockId,
    ) -> Result<Opened<ResultStreamBox, RuntimeToolResults>, ThreadError> {
        let who = self.who(agent)?;
        let projected = self.projected(thread, &who).await?;
        if let Ok(held) = projected.entry(&entry) {
            self.discard(agent, entry);
            return match held {
                EntryView::Turn(TurnEntry::RuntimeToolResults(results)) => {
                    Ok(Opened::Held(results.clone()))
                }
                _ => Err(refusal("the thread holds the entry as another kind of entry").into()),
            };
        }
        Self::check_open_target(&projected, &who, &target)?;
        match projected.turn(&target.id()) {
            Ok(Turn::AITurn(ai)) if ai.state().outstanding_runtime_calls().contains(&call) => {}
            _ => return Err(refusal("the tool call is not pending").into()),
        }
        let draft = br_llm_messages::ResponseDraft::new(target.id(), entry);
        let previous = self
            .core
            .objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                entry,
                OpenObject {
                    agent,
                    alive: Arc::new(AtomicBool::new(true)),
                    draft: Arc::new(Mutex::new(draft)),
                },
            );
        drop(previous);
        Ok(Opened::Stream(Box::new(DiscardingResultStream)))
    }

    pub(crate) async fn complete_tool_result(
        &self,
        thread: &ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        entry: EntryId,
        result: RuntimeToolResult,
    ) -> Result<Completed<RuntimeToolResults>, ThreadError> {
        let who = self.who(agent)?;
        let projected = self.projected(thread, &who).await?;
        if let Ok(held) = projected.entry(&entry) {
            self.discard(agent, entry);
            return match held {
                EntryView::Turn(TurnEntry::RuntimeToolResults(results)) => {
                    Ok(Completed::Held(results.clone()))
                }
                _ => Err(refusal("the thread holds the entry as another kind of entry").into()),
            };
        }
        Self::check_open_target(&projected, &who, &target)?;
        let TurnTarget::Ai { id, author } = &target else {
            return Err(refusal("a tool result goes into an AI turn").into());
        };
        let recorded =
            TurnEntry::from(RuntimeToolResults::new(entry, vec![result]).map_err(refusal)?);
        Self::check_entry(&projected, &who, *id, author, &recorded)?;
        let open = self.stream_for(thread, &who, *id, &projected).await?;
        open.handle.entry(&recorded).await.map_err(from_threads)?;
        self.discard(agent, entry);
        Ok(Completed::Added)
    }
}
