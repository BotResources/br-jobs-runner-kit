use std::sync::Arc;
use std::sync::atomic::Ordering;

use br_agentic_contract::{ThreadRef, TurnTarget};
use br_llm_messages::{AgentId, Author, ConversationId, EntryId, ProjectedThread, Turn, TurnId};
use br_runner_threads::{ContextContent, ContextQuery};
use uuid::Uuid;

use super::ToolkitThreadAccess;
use super::streams::{OpenTurn, Opening, Owner};
use crate::agents::AgentKey;
use crate::entries::MailOrigin;
use crate::error::{PortError, PortErrorCode, from_threads, refusal};

pub(crate) struct Who {
    pub run: Uuid,
    pub key: AgentKey,
    pub agent: AgentId,
    pub author: Author,
}

impl ToolkitThreadAccess {
    pub(crate) fn who(&self, agent: AgentId) -> Result<Who, PortError> {
        self.core.link()?;
        let (run, key) = self.core.runs.live_agent(agent)?;
        let author = self.core.author(&key)?;
        Ok(Who {
            run,
            key,
            agent,
            author,
        })
    }

    pub(crate) async fn projected(
        &self,
        thread: &ThreadRef,
        who: &Who,
    ) -> Result<ProjectedThread, PortError> {
        let link = self.core.link()?;
        let query = ContextQuery {
            thread: *thread,
            agent: Some(who.agent),
        };
        let found = link
            .threads
            .read_context(&query)
            .await
            .map_err(from_threads)?;
        match found.map(|context| context.content) {
            Some(ContextContent::Projected(projected)) => Ok(projected),
            Some(_) => Err(PortError::new(
                PortErrorCode::ThreadsFailed,
                "threads gave no projected thread",
            )),
            None => Err(PortError::new(
                PortErrorCode::ThreadsFailed,
                format!("the thread {thread} does not exist"),
            )),
        }
    }

    pub(crate) fn origin(
        &self,
        thread: &ThreadRef,
    ) -> impl Fn(AgentId) -> Result<MailOrigin, PortError> + use<> {
        let core = Arc::clone(&self.core);
        let thread = *thread;
        move |sender| {
            let source = ConversationId::from_uuid(thread.as_uuid()).map_err(refusal)?;
            let key = core
                .runs
                .with(|state| state.agents.get(&sender).map(|record| record.key.clone()));
            let author = match key.and_then(|key| core.author(&key).ok()) {
                Some(author) => author,
                None => Author::new(sender.as_uuid().to_string()).map_err(refusal)?,
            };
            Ok(MailOrigin { author, source })
        }
    }

    pub(crate) fn parent_of(projected: &ProjectedThread) -> Option<Uuid> {
        projected.turns().last().map(|turn| turn.id().as_uuid())
    }

    pub(crate) fn check_open_target(
        projected: &ProjectedThread,
        who: &Who,
        target: &TurnTarget,
    ) -> Result<(), PortError> {
        match (projected.turn(&target.id()), target) {
            (Ok(Turn::AITurn(ai)), TurnTarget::Ai { author, .. }) => {
                if ai.agent() != who.agent || ai.author() != author {
                    return Err(refusal("the AI turn belongs to another agent or author"));
                }
                if ai.is_closed() {
                    return Err(refusal("the AI turn is closed"));
                }
                Ok(())
            }
            (Ok(Turn::AITurn(_)), TurnTarget::Input(_)) => {
                Err(refusal("the turn is an AI turn, not an input turn"))
            }
            (Ok(_), _) => Err(refusal("the turn is an input turn, not an AI turn")),
            (Err(_), TurnTarget::Ai { .. }) => Ok(()),
            (Err(_), TurnTarget::Input(_)) => Err(refusal("the input turn does not exist")),
        }
    }

    pub(crate) async fn stream_for(
        &self,
        thread: &ThreadRef,
        who: &Who,
        turn: TurnId,
        projected: &ProjectedThread,
    ) -> Result<Arc<OpenTurn>, PortError> {
        if let Some(open) = self.core.streams.get(turn) {
            return Ok(open);
        }
        if projected.turn(&turn).is_ok() {
            return Err(refusal("the toolkit holds no stream for the AI turn"));
        }
        let link = self.core.link()?;
        self.core
            .streams
            .ensure(
                &link.threads,
                &self.core.runs,
                Owner {
                    run: who.run,
                    node: who.key.node.clone(),
                    agent: who.agent,
                    author: who.author.clone(),
                },
                Opening {
                    thread: *thread,
                    turn,
                    parent: Self::parent_of(projected),
                },
            )
            .await
    }

    pub(crate) fn discard(&self, agent: AgentId, entry: EntryId) {
        let mut objects = self
            .core
            .objects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if objects.get(&entry).is_some_and(|open| open.agent == agent)
            && let Some(open) = objects.remove(&entry)
        {
            open.alive.store(false, Ordering::SeqCst);
        }
    }
}
