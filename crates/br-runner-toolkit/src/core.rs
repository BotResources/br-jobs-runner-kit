use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use br_agentic_contract::AgentLabel;
use br_llm_messages::{AgentId, Author, EntryId, ResponseDraft};
use br_runner_threads::ThreadsClient;
use uuid::Uuid;

use crate::agents::{AgentKey, AgentRegistrar, Declared};
use crate::error::{PortError, PortErrorCode};
use crate::runs::Runs;
use crate::thread_access::Streams;

pub(crate) struct Link {
    pub registrar: AgentRegistrar,
    pub threads: ThreadsClient,
}

#[derive(Clone)]
pub(crate) struct NodeSpec {
    pub authors: HashMap<AgentLabel, Author>,
    pub inbox: Option<AgentLabel>,
}

pub(crate) struct OpenObject {
    pub agent: AgentId,
    pub alive: Arc<AtomicBool>,
    pub draft: Arc<Mutex<ResponseDraft>>,
}

pub(crate) struct Core {
    pub runs: Runs,
    pub streams: Streams,
    pub objects: Mutex<HashMap<EntryId, OpenObject>>,
    nodes: Mutex<HashMap<String, NodeSpec>>,
    can_restart: Option<bool>,
    link: OnceLock<Link>,
}

impl Core {
    pub(crate) fn new(can_restart: Option<bool>) -> Self {
        Self {
            runs: Runs::default(),
            streams: Streams::default(),
            objects: Mutex::default(),
            nodes: Mutex::default(),
            can_restart,
            link: OnceLock::new(),
        }
    }

    pub(crate) fn drop_objects_of(&self, agents: &[AgentId]) {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, open| {
                let owned = agents.contains(&open.agent);
                if owned {
                    open.alive.store(false, Ordering::SeqCst);
                }
                !owned
            });
    }

    pub(crate) fn declare(&self, node: &str, spec: NodeSpec) {
        self.nodes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(node.to_owned(), spec);
    }

    pub(crate) fn bind(&self, link: Link) -> Result<(), PortError> {
        self.link
            .set(link)
            .map_err(|_| PortError::new(PortErrorCode::NotStarted, "the ports are already bound"))
    }

    pub(crate) fn link(&self) -> Result<&Link, PortError> {
        self.link.get().ok_or_else(|| {
            PortError::new(
                PortErrorCode::NotStarted,
                "a port call came before the runner started",
            )
        })
    }

    pub(crate) fn node(&self, node: &str) -> Result<NodeSpec, PortError> {
        self.nodes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(node)
            .cloned()
            .ok_or_else(|| {
                PortError::new(
                    PortErrorCode::NoAuthorName,
                    format!("the node {node} has no port set"),
                )
            })
    }

    pub(crate) fn author(&self, key: &AgentKey) -> Result<Author, PortError> {
        self.node(&key.node)?
            .authors
            .get(&key.label)
            .cloned()
            .ok_or_else(|| {
                PortError::new(
                    PortErrorCode::NoAuthorName,
                    format!("no author name for {} of {}", key.label, key.node),
                )
            })
    }

    fn can_restart(&self) -> bool {
        self.can_restart.unwrap_or_else(|| {
            self.nodes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .any(|spec| spec.inbox.is_some())
        })
    }

    pub(crate) async fn identity(
        &self,
        run: Uuid,
        key: AgentKey,
    ) -> Result<(AgentId, Author), PortError> {
        let link = self.link()?;
        let spec = self.node(&key.node)?;
        let author = self.author(&key)?;
        let declared = Declared {
            has_inbox: spec.inbox.as_ref() == Some(&key.label),
            can_restart: self.can_restart(),
        };
        let agent = self
            .runs
            .identity(run, key, declared, link.registrar.clone())
            .await?;
        Ok((agent, author))
    }
}
