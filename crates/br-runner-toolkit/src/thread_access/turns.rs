use br_agentic_contract::{Incoming, Stored, ThreadError, ThreadRef, TurnTarget};
use br_llm_messages::{ClosureReason, InputTurn, Turn, TurnClosure, TurnEntry, TurnId};
use br_runner_threads::{Closing, ClosureKind, MessageStatus, WholeMessage};
use uuid::Uuid;

use super::ToolkitThreadAccess;
use crate::entries::incoming_entry;
use crate::error::{PortError, PortErrorCode, from_threads, refusal};

fn kind_of(reason: &ClosureReason) -> Result<(ClosureKind, Option<String>), PortError> {
    let detail = reason.detail().map(str::to_owned);
    match reason {
        ClosureReason::Completed { .. } => Ok((ClosureKind::Completed, detail)),
        ClosureReason::Cancelled { .. } => Ok((ClosureKind::Cancelled, detail)),
        ClosureReason::Interrupted { .. } => Ok((ClosureKind::Interrupted, detail)),
        ClosureReason::BudgetExhausted { .. } => Ok((ClosureKind::BudgetExhausted, detail)),
        ClosureReason::Failed { .. } => Ok((ClosureKind::Failed, detail)),
        _ => Err(refusal("the toolkit does not know this closure reason")),
    }
}

impl ToolkitThreadAccess {
    pub(crate) async fn input_turn(
        &self,
        thread: &ThreadRef,
        agent: br_llm_messages::AgentId,
        turn: TurnId,
        items: Vec<Incoming>,
    ) -> Result<Stored, ThreadError> {
        let who = self.who(agent)?;
        let projected = self.projected(thread, &who).await?;
        let fresh: Vec<&Incoming> = items
            .iter()
            .filter(|item| projected.entry(&item.id()).is_err())
            .collect();
        if fresh.is_empty() {
            return Ok(Stored::Held);
        }
        if projected.turn(&turn).is_ok() {
            return Err(refusal("the turn identity is held with other content").into());
        }
        let origin = self.origin(thread);
        let entries = fresh
            .into_iter()
            .map(|item| incoming_entry(item, &origin))
            .collect::<Result<Vec<_>, _>>()?;
        let input = Turn::from(InputTurn::new(turn, entries).map_err(refusal)?);
        if let Some(error) = projected.would_refuse_turn(&input) {
            return Err(refusal(error).into());
        }
        let link = self.core.link()?;
        link.threads
            .write_message(&WholeMessage {
                command_id: Uuid::now_v7(),
                thread: *thread,
                parent_message_id: Self::parent_of(&projected),
                agent_id: agent,
                author_name: who.author.as_str().to_owned(),
                job_id: None,
                acting_user_id: None,
                impersonator_id: None,
                turn: input,
                annotations: Vec::new(),
                wait: None,
            })
            .await
            .map_err(from_threads)?;
        Ok(Stored::Added)
    }

    pub(crate) async fn incoming(
        &self,
        thread: &ThreadRef,
        agent: br_llm_messages::AgentId,
        target: TurnTarget,
        item: Incoming,
    ) -> Result<Stored, ThreadError> {
        let who = self.who(agent)?;
        let projected = self.projected(thread, &who).await?;
        if projected.entry(&item.id()).is_ok() {
            return Ok(Stored::Held);
        }
        let TurnTarget::Ai { id, author } = &target else {
            return Err(refusal("an input turn that threads holds whole takes no entry").into());
        };
        Self::check_open_target(&projected, &who, &target)?;
        let incoming = incoming_entry(&item, self.origin(thread))?;
        let entry = TurnEntry::try_from(incoming).map_err(refusal)?;
        Self::check_entry(&projected, &who, *id, author, &entry)?;
        let open = self.stream_for(thread, &who, *id, &projected).await?;
        open.handle.entry(&entry).await.map_err(from_threads)?;
        Ok(Stored::Added)
    }

    pub(crate) async fn close(
        &self,
        thread: &ThreadRef,
        agent: br_llm_messages::AgentId,
        turn: TurnId,
        closure: TurnClosure,
    ) -> Result<Stored, ThreadError> {
        let who = self.who(agent)?;
        let projected = self.projected(thread, &who).await?;
        let handle = self.core.streams.get(turn);
        match projected.turn(&turn) {
            Ok(Turn::AITurn(ai)) if ai.agent() != agent => {
                return Err(refusal("the AI turn belongs to another agent").into());
            }
            Ok(Turn::AITurn(ai)) if ai.is_closed() => return Ok(Stored::Held),
            Ok(Turn::AITurn(_)) => {}
            Ok(_) => return Err(refusal("the turn is not an AI turn").into()),
            Err(_) if handle.is_some() => {}
            Err(_) => return Err(refusal("the AI turn does not exist").into()),
        }
        let (kind, detail) = kind_of(closure.reason())?;
        let link = self.core.link()?;
        let confirmed = link
            .threads
            .close_message(
                &Closing {
                    command_id: Uuid::now_v7(),
                    thread: *thread,
                    message_id: turn.as_uuid(),
                    kind,
                    detail,
                    acting_user_id: None,
                    wait: None,
                },
                handle.as_ref().map(|open| open.handle.as_ref()),
            )
            .await
            .map_err(from_threads)?;
        if matches!(kind, ClosureKind::Completed | ClosureKind::BudgetExhausted)
            && confirmed.status == Some(MessageStatus::Failed)
        {
            return Err(PortError::new(
                PortErrorCode::ThreadsFailed,
                "threads closed the message as failed",
            )
            .into());
        }
        self.core.streams.forget(turn);
        Ok(Stored::Added)
    }

    pub(crate) fn check_entry(
        projected: &br_llm_messages::ProjectedThread,
        who: &super::context::Who,
        turn: TurnId,
        author: &br_llm_messages::Author,
        entry: &TurnEntry,
    ) -> Result<(), PortError> {
        let refused = match projected.turn(&turn) {
            Ok(_) => projected.would_refuse_entry(&turn, entry),
            Err(_) => {
                let mut preview = projected.clone();
                let opened = br_llm_messages::AITurn::open(turn, who.agent, author.clone());
                preview
                    .push_turn(Turn::from(opened))
                    .and_then(|()| preview.push_entry(&turn, entry.clone()))
                    .err()
            }
        };
        refused.map_or(Ok(()), |error| Err(refusal(error)))
    }
}
