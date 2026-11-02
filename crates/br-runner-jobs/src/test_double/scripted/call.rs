use serde_json::Value;
use uuid::Uuid;

use crate::arrival::ArrivalSettings;
use crate::backend::{ChildCommand, Fact, LogRecord};
use crate::model::AgentDeclaration;

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Graphql {
        document: String,
        variables: Value,
    },
    Claim {
        claim_id: Uuid,
    },
    Started {
        run_id: Uuid,
        start_id: Uuid,
    },
    Fact(Fact),
    Log(LogRecord),
    PresencePut,
    PresenceDelete,
    Register {
        run_id: Uuid,
        agents: Vec<AgentDeclaration>,
    },
    ListAgents {
        run_id: Uuid,
    },
    MarkRead {
        run_id: Uuid,
        item_ids: Vec<Uuid>,
    },
    Command(ChildCommand),
    ArrivalFeed(ArrivalSettings),
}
