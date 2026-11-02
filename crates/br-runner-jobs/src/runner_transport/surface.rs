use std::time::Duration;

use contract_jobs::reason::Reason;
use contract_jobs::runner::{ClaimV2, WIRE_VERSION_V2};
use contract_jobs::runner_surface::{
    AgentDeclaration as WireAgent, AgentState, ListAgents, ListAgentsReply, MarkRead,
    RegisterAgents, SURFACE_WIRE_VERSION, SurfaceReason, SurfaceReply, list_agents_subject,
    mark_read_subject, register_agents_subject,
};
use contract_jobs::runner_transport::claim_v2_subject;
use serde::Serialize;
use uuid::Uuid;

use super::Live;
use crate::error::{Error, Operation};
use crate::from_connection;
use crate::model::{AgentDeclaration, ListedAgent};
use crate::reason::RefusalReason;
use crate::runner_type::RunnerType;

fn millis(wait: Duration) -> u64 {
    u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value).map_err(|e| Error::invalid(e.to_string()))
}

fn word(reason: Option<Reason<SurfaceReason>>) -> RefusalReason {
    match reason {
        Some(Reason::Unknown(text)) => RefusalReason::from_word(&text),
        Some(Reason::Known(known)) => serde_json::to_value(known)
            .ok()
            .and_then(|value| value.as_str().map(RefusalReason::from_word))
            .unwrap_or_else(|| RefusalReason::from_word("unspecified")),
        None => RefusalReason::from_word("unspecified"),
    }
}

impl Live {
    async fn request(
        &self,
        subject: String,
        body: Vec<u8>,
        wait: Duration,
        operation: Operation,
    ) -> Result<Vec<u8>, Error> {
        self.connection
            .bus_request(&subject, body.into(), wait)
            .await
            .map(|reply| reply.to_vec())
            .map_err(|error| from_connection::error(error, operation))
    }

    pub(super) async fn claim_request(
        &self,
        claim_id: Uuid,
        claim_wait: Duration,
    ) -> Result<Vec<u8>, Error> {
        let body = encode(&ClaimV2 {
            version: WIRE_VERSION_V2,
            instance_key: self.instance_key.clone(),
            claim_id,
            wait_ms: millis(claim_wait),
        })?;
        self.request(
            claim_v2_subject(&self.segments.runner_type),
            body,
            claim_wait.saturating_add(self.reply_wait),
            Operation::Claim,
        )
        .await
    }

    pub(super) async fn register(
        &self,
        run_id: Uuid,
        agents: Vec<AgentDeclaration>,
        wait: Duration,
    ) -> Result<(), Error> {
        let body = encode(&RegisterAgents {
            version: SURFACE_WIRE_VERSION,
            run_id,
            instance_key: self.instance_key.clone(),
            agents: agents
                .into_iter()
                .map(|agent| WireAgent {
                    agent_id: agent.agent_id,
                    has_inbox: agent.has_inbox,
                    declares_restart: agent.can_restart,
                })
                .collect(),
        })?;
        let subject = register_agents_subject(&self.segments.runner_type);
        let reply = self
            .request(subject, body, wait, Operation::Registration)
            .await?;
        accepted(&reply, Operation::Registration)
    }

    pub(super) async fn mark(
        &self,
        run_id: Uuid,
        item_ids: Vec<Uuid>,
        wait: Duration,
    ) -> Result<(), Error> {
        let body = encode(&MarkRead {
            version: SURFACE_WIRE_VERSION,
            run_id,
            instance_key: self.instance_key.clone(),
            item_ids,
        })?;
        let subject = mark_read_subject(&self.segments.runner_type);
        let reply = self
            .request(subject, body, wait, Operation::ReadMarker)
            .await?;
        accepted(&reply, Operation::ReadMarker)
    }

    pub(super) async fn list(
        &self,
        run_id: Uuid,
        wait: Duration,
    ) -> Result<Vec<ListedAgent>, Error> {
        let body = encode(&ListAgents {
            version: SURFACE_WIRE_VERSION,
            run_id,
            instance_key: self.instance_key.clone(),
        })?;
        let subject = list_agents_subject(&self.segments.runner_type);
        let reply = self
            .request(subject, body, wait, Operation::AgentList)
            .await?;
        let reply: ListAgentsReply = serde_json::from_slice(&reply)
            .map_err(|e| Error::unreadable_json("the agent list reply", &e))?;
        if !reply.accepted {
            return Err(Error::refused(
                Operation::AgentList,
                word(reply.reason),
                reply.message.unwrap_or_default(),
            ));
        }
        let agents = reply
            .agents
            .ok_or_else(|| Error::Unreadable("the agent list reply holds no agents".to_owned()))?;
        agents
            .into_iter()
            .map(|agent| {
                Ok(ListedAgent {
                    agent_id: agent.agent_id,
                    run_id: agent.run_id,
                    runner_type: RunnerType::new(&agent.runner_type).map_err(|_| {
                        Error::Unreadable(
                            "the agent list holds a runner type the wire refuses".to_owned(),
                        )
                    })?,
                    running: agent.state == AgentState::Running,
                    has_inbox: agent.has_inbox,
                    mail_address: agent.mail_address,
                    restartable_now: agent.restartable_now,
                })
            })
            .collect()
    }
}

fn accepted(reply: &[u8], operation: Operation) -> Result<(), Error> {
    let reply: SurfaceReply = serde_json::from_slice(reply)
        .map_err(|e| Error::unreadable_json(&format!("the {} reply", operation.as_str()), &e))?;
    if reply.accepted {
        return Ok(());
    }
    Err(Error::refused(
        operation,
        word(reply.reason),
        reply.message.unwrap_or_default(),
    ))
}
