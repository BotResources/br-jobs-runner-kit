use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use br_agentic_contract::ThreadRef;
use br_llm_messages::EntryId;

use crate::queue::Gate;

pub type GateFuture = Pin<Box<dyn Future<Output = Gate> + Send + 'static>>;

pub type MarkGate = Arc<dyn Fn(ThreadRef, EntryId) -> GateFuture + Send + Sync>;

pub fn mark_gate<F, Fut>(gate: F) -> MarkGate
where
    F: Fn(ThreadRef, EntryId) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Gate> + Send + 'static,
{
    Arc::new(move |thread, entry| Box::pin(gate(thread, entry)))
}

pub fn mark_gate_stored() -> MarkGate {
    mark_gate(|_, _| std::future::ready(Gate::Stored))
}
