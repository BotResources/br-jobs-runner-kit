use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use br_agentic_contract::ThreadRef;
use br_llm_messages::{AgentId, Author, TurnId};
use br_runner_threads::{OpenStream, StreamHandle, ThreadsClient};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::error::{PortError, from_threads};
use crate::runs::Runs;

pub(crate) const KEEPALIVE_EVERY: Duration = Duration::from_secs(300);

pub(crate) struct OpenTurn {
    run: Uuid,
    pub handle: Arc<StreamHandle>,
    keepalive: JoinHandle<()>,
}

impl Drop for OpenTurn {
    fn drop(&mut self) {
        self.keepalive.abort();
    }
}

#[derive(Default)]
pub(crate) struct Streams {
    turns: Mutex<HashMap<TurnId, Arc<OpenTurn>>>,
    opening: Mutex<HashMap<TurnId, Arc<tokio::sync::Mutex<()>>>>,
}

impl Streams {
    pub(crate) fn get(&self, turn: TurnId) -> Option<Arc<OpenTurn>> {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&turn)
            .cloned()
    }

    pub(crate) fn forget(&self, turn: TurnId) {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&turn);
    }

    pub(crate) fn forget_run(&self, run: Uuid) {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, open| open.run != run);
    }

    pub(crate) async fn ensure(
        &self,
        client: &ThreadsClient,
        runs: &Runs,
        owner: Owner,
        open: Opening,
    ) -> Result<Arc<OpenTurn>, PortError> {
        let gate = Arc::clone(
            self.opening
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(open.turn)
                .or_default(),
        );
        let _guard = gate.lock().await;
        let turn = open.turn;
        let result = self.open_once(client, runs, owner, open).await;
        let mut gates = self.opening.lock().unwrap_or_else(PoisonError::into_inner);
        if Arc::strong_count(&gate) <= 2 {
            gates.remove(&turn);
        }
        result
    }

    async fn open_once(
        &self,
        client: &ThreadsClient,
        runs: &Runs,
        owner: Owner,
        open: Opening,
    ) -> Result<Arc<OpenTurn>, PortError> {
        if let Some(existing) = self.get(open.turn) {
            return Ok(existing);
        }
        let request = OpenStream {
            command_id: Uuid::now_v7(),
            thread: open.thread,
            message_id: open.turn,
            parent_message_id: open.parent,
            agent_id: owner.agent,
            author_name: owner.author.as_str().to_owned(),
            job_id: None,
            acting_user_id: None,
            wait: None,
        };
        let handle = Arc::new(client.open_stream(&request).await.map_err(from_threads)?);
        let run = owner.run;
        let keepalive = tokio::spawn(beat(Arc::clone(&handle), runs.clone(), run, owner.node));
        let turn = Arc::new(OpenTurn {
            run,
            handle,
            keepalive,
        });
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(open.turn, Arc::clone(&turn));
        Ok(turn)
    }
}

pub(crate) struct Owner {
    pub run: Uuid,
    pub node: String,
    pub agent: AgentId,
    pub author: Author,
}

pub(crate) struct Opening {
    pub thread: ThreadRef,
    pub turn: TurnId,
    pub parent: Option<Uuid>,
}

async fn beat(handle: Arc<StreamHandle>, runs: Runs, run: Uuid, node: String) {
    loop {
        tokio::time::sleep(KEEPALIVE_EVERY).await;
        if !runs.is_in_flight(run) || !runs.has_live_instance(run, &node) {
            return;
        }
        if let Err(error) = handle.keepalive().await {
            tracing::error!(%error, message = %handle.message_id(), "keepalive not sent");
        }
    }
}
