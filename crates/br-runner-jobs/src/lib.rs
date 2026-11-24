mod adapter;

pub use adapter::{
    AgentPortAdapter, GateFuture, IncomingPortAdapter, MailPortAdapter, MarkGate, NodeEntry,
    mark_gate, mark_gate_stored,
};
