mod context;
mod responses;
mod results;
mod streams;
mod turns;

use std::sync::Arc;

use br_agentic_contract::{
    Completed, Incoming, Opened, PortFault, ResponseStream, ResultStreamBox, Stored, ThreadAccess,
    ThreadError, ThreadRef, TurnTarget,
};
use br_llm_clients_core::BoxFuture;
use br_llm_messages::{
    AgentId, BlockId, EntryId, ModelResponse, ProjectedThread, RuntimeToolResult,
    RuntimeToolResults, TurnClosure, TurnId,
};

use crate::core::Core;

pub(crate) use streams::Streams;

pub struct ToolkitThreadAccess {
    core: Arc<Core>,
}

impl ToolkitThreadAccess {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }
}

impl ThreadAccess for ToolkitThreadAccess {
    fn load<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
    ) -> BoxFuture<'a, Result<ProjectedThread, PortFault>> {
        Box::pin(async move {
            let who = self.who(agent)?;
            Ok(self.projected(thread, &who).await?)
        })
    }

    fn add_input_turn<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
        turn: TurnId,
        items: Vec<Incoming>,
    ) -> BoxFuture<'a, Result<Stored, ThreadError>> {
        Box::pin(async move { self.input_turn(thread, agent, turn, items).await })
    }

    fn add_incoming<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        item: Incoming,
    ) -> BoxFuture<'a, Result<Stored, ThreadError>> {
        Box::pin(async move { self.incoming(thread, agent, target, item).await })
    }

    fn close_turn<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
        turn: TurnId,
        closure: TurnClosure,
    ) -> BoxFuture<'a, Result<Stored, ThreadError>> {
        Box::pin(async move { self.close(thread, agent, turn, closure).await })
    }

    fn open_response<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        entry: EntryId,
    ) -> BoxFuture<'a, Result<Opened<ResponseStream, ModelResponse>, ThreadError>> {
        Box::pin(async move {
            self.open_response_stream(thread, agent, target, entry)
                .await
        })
    }

    fn complete_response<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        response: ModelResponse,
    ) -> BoxFuture<'a, Result<Completed<ModelResponse>, ThreadError>> {
        Box::pin(async move {
            self.complete_model_response(thread, agent, target, response)
                .await
        })
    }

    fn discard_response<'a>(
        &'a self,
        _thread: &'a ThreadRef,
        agent: AgentId,
        entry: EntryId,
    ) -> BoxFuture<'a, Result<(), ThreadError>> {
        Box::pin(async move {
            self.discard(agent, entry);
            Ok(())
        })
    }

    fn open_result<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        entry: EntryId,
        call: BlockId,
    ) -> BoxFuture<'a, Result<Opened<ResultStreamBox, RuntimeToolResults>, ThreadError>> {
        Box::pin(async move {
            self.open_result_stream(thread, agent, target, entry, call)
                .await
        })
    }

    fn complete_result<'a>(
        &'a self,
        thread: &'a ThreadRef,
        agent: AgentId,
        target: TurnTarget,
        entry: EntryId,
        result: RuntimeToolResult,
    ) -> BoxFuture<'a, Result<Completed<RuntimeToolResults>, ThreadError>> {
        Box::pin(async move {
            self.complete_tool_result(thread, agent, target, entry, result)
                .await
        })
    }

    fn discard_result<'a>(
        &'a self,
        _thread: &'a ThreadRef,
        agent: AgentId,
        entry: EntryId,
    ) -> BoxFuture<'a, Result<(), ThreadError>> {
        Box::pin(async move {
            self.discard(agent, entry);
            Ok(())
        })
    }
}
