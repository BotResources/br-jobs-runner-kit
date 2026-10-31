use std::sync::Arc;

use br_graph_contract::{
    CancelView, CreateFuture, Edge, Event, Fields, Graph, GraphBuilder, Instance, InstanceId, Node,
    Reason, State, Target,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Flow {
    pub steps: Vec<String>,
}

pub struct Add(pub String);

impl Fields for Flow {
    type Update = Add;

    fn empty_update() -> Add {
        Add(String::new())
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: Add) -> Result<(), Reason> {
        if !update.0.is_empty() {
            self.steps.push(update.0);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub enum Behavior {
    Result(&'static str),
    Fail(&'static str),
    Refuse(&'static str),
    UntilCancelled,
    UntilReleased(Arc<Notify>, &'static str),
}

#[derive(Clone)]
pub struct Agent {
    pub lookup: br_runner_toolkit::InstanceLookup,
    pub label: &'static str,
    pub inbox: bool,
    pub seen_run: Arc<std::sync::Mutex<Option<uuid::Uuid>>>,
}

pub struct Worker {
    pub name: String,
    pub behavior: Behavior,
    pub agent: Option<Agent>,
}

impl Node<Flow> for Worker {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> &str {
        "programmatic"
    }

    fn subkind(&self) -> &str {
        "worker"
    }

    fn create(&self, cancel: CancelView, id: InstanceId) -> CreateFuture<'_, Flow> {
        let behavior = self.behavior.clone();
        let agent = self.agent.clone();
        let name = self.name.clone();
        Box::pin(async move {
            let (instance, mut reader, sender) = Instance::<Flow>::channel(id);
            tokio::spawn(async move {
                if let Some(agent) = agent {
                    let book = agent.lookup.book(id).await.expect("a book");
                    let context = agent.lookup.run_context(id).await.expect("a context");
                    *agent.seen_run.lock().expect("lock") = Some(context.run_id());
                    let label = br_agentic_contract::AgentLabel::new(agent.label).expect("label");
                    let entry = if agent.inbox {
                        br_runner_jobs::NodeEntry::with_inbox(
                            name,
                            agent.lookup.book_lookup(),
                            label.clone(),
                            br_runner_jobs::mark_gate_stored(),
                        )
                    } else {
                        br_runner_jobs::NodeEntry::new(name, agent.lookup.book_lookup())
                    };
                    entry.agent_id(&book, &label).expect("an agent id");
                }
                let _ = reader.next().await;
                let event = match behavior {
                    Behavior::Result(text) => Event::Result(Add(text.to_owned())),
                    Behavior::Fail(text) => Event::failed(text),
                    Behavior::Refuse(text) => Event::refused(text),
                    Behavior::UntilCancelled => {
                        cancel.triggered().await;
                        Event::Cancelled
                    }
                    Behavior::UntilReleased(gate, text) => {
                        gate.notified().await;
                        Event::Result(Add(text.to_owned()))
                    }
                };
                sender.finish(event);
            });
            Ok(instance)
        })
    }
}

struct Next {
    source: String,
    targets: Vec<Target>,
}

impl Edge<Flow> for Next {
    fn name(&self) -> &str {
        &self.source
    }

    fn source(&self) -> &str {
        &self.source
    }

    fn targets(&self) -> &[Target] {
        &self.targets
    }

    fn evaluate(&self, _state: &State<Flow>) -> Result<Vec<Target>, br_graph_contract::Cause> {
        Ok(self.targets.clone())
    }
}

pub fn node(name: &str, behavior: Behavior) -> Arc<dyn Node<Flow>> {
    agent_node(name, behavior, None)
}

pub fn agent_node(name: &str, behavior: Behavior, agent: Option<Agent>) -> Arc<dyn Node<Flow>> {
    Arc::new(Worker {
        name: name.to_owned(),
        behavior,
        agent,
    })
}

pub fn agent_graph(behavior: Behavior, agent: Agent) -> Arc<Graph<Flow>> {
    Arc::new(
        GraphBuilder::<Flow>::new("1")
            .entry("work")
            .node(agent_node("work", behavior, Some(agent)))
            .edge(Arc::new(Next {
                source: "work".to_owned(),
                targets: vec![Target::END],
            }))
            .build()
            .expect("a graph"),
    )
}

pub fn chain(nodes: Vec<(&str, Behavior)>) -> Arc<Graph<Flow>> {
    let mut builder = GraphBuilder::<Flow>::new("1").entry(nodes[0].0);
    for (position, (name, behavior)) in nodes.iter().enumerate() {
        let target = nodes
            .get(position + 1)
            .map_or(Target::END, |next| Target::node(next.0));
        builder = builder
            .node(node(name, behavior.clone()))
            .edge(Arc::new(Next {
                source: (*name).to_owned(),
                targets: vec![target],
            }));
    }
    Arc::new(builder.build().expect("a graph"))
}

pub fn single(behavior: Behavior) -> Arc<Graph<Flow>> {
    chain(vec![("work", behavior)])
}
