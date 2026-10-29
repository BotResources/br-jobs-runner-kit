use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use br_agentic_contract::{Agent, AgentLabel, AgentPort, PortFault, ThreadRef};
use br_graph_contract::{Fields, InstanceId};
use br_llm_clients_core::BoxFuture;
use br_llm_messages::Author;
use uuid::Uuid;

use br_runner_threads::ThreadsClient;

use crate::agents::{AgentKey, AgentRegistrar};
use crate::core::{Core, Link, NodeSpec};
use crate::error::PortError;
use crate::run_handle::RunHandle;
use crate::saved_state::{SavedState, SavedStateError};
use crate::thread_access::ToolkitThreadAccess;

#[derive(Debug, Clone, Default)]
pub struct PortSetRequest {
    authors: HashMap<AgentLabel, Author>,
    inbox: Option<AgentLabel>,
}

impl PortSetRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn author(mut self, label: AgentLabel, author: Author) -> Self {
        self.authors.insert(label, author);
        self
    }

    pub fn inbox(mut self, label: AgentLabel) -> Self {
        self.inbox = Some(label);
        self
    }
}

pub struct PortSets {
    core: Arc<Core>,
}

pub struct PortSet<F: Fields> {
    pub agent: Arc<ToolkitAgentPort<F>>,
    pub thread_access: Arc<ToolkitThreadAccess>,
}

pub struct ToolkitAgentPort<F: Fields> {
    core: Arc<Core>,
    fields: PhantomData<fn() -> F>,
}

impl PortSets {
    pub fn new(can_restart: Option<bool>) -> Self {
        Self {
            core: Arc::new(Core::new(can_restart)),
        }
    }

    pub fn port_set<F: Fields>(&self, node: &str, request: PortSetRequest) -> PortSet<F> {
        self.core.declare(
            node,
            NodeSpec {
                authors: request.authors,
                inbox: request.inbox,
            },
        );
        PortSet {
            agent: Arc::new(ToolkitAgentPort {
                core: Arc::clone(&self.core),
                fields: PhantomData,
            }),
            thread_access: Arc::new(ToolkitThreadAccess::new(Arc::clone(&self.core))),
        }
    }

    pub fn bind(&self, threads: ThreadsClient, registrar: AgentRegistrar) -> Result<(), PortError> {
        self.core.bind(Link { registrar, threads })
    }

    pub fn begin_run(
        &self,
        run: Uuid,
        restored: Option<&SavedState>,
    ) -> Result<RunHandle, SavedStateError> {
        RunHandle::begin(Arc::clone(&self.core), run, restored)
    }
}

pub fn thread_reference(thread: Uuid) -> ThreadRef {
    ThreadRef::new(thread)
}

impl<F: Fields> AgentPort<F> for ToolkitAgentPort<F> {
    fn agent<'a>(
        &'a self,
        _snapshot: &'a F,
        instance: InstanceId,
        label: &'a AgentLabel,
    ) -> BoxFuture<'a, Result<Agent, PortFault>> {
        Box::pin(async move {
            self.core.link()?;
            let at = self.core.runs.instance(instance).await?;
            let key = AgentKey::new(at.node, label.clone());
            let (id, author) = self.core.identity(at.run, key).await?;
            Ok(Agent {
                id,
                author: Some(author),
            })
        })
    }
}
