use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use br_graph_contract::{
    CancelView, Cause, CreateFuture, Edge, Event, Fields, Graph, GraphBuilder, Instance,
    InstanceId, Node, Reason, State, Target,
};
use br_runner_toolkit::{InstanceLookup, LifecycleHook, lifecycle_hook};
use serde::{Deserialize, Serialize, Serializer};

const QUIET: u8 = 0;
const PANIC: u8 = 1;
const ERROR: u8 = 2;

#[derive(Clone, Default)]
pub struct Trap(Arc<AtomicU8>);

impl Trap {
    fn armed_by_the_hook(&self, mode: u8) -> LifecycleHook {
        let trap = self.clone();
        let first = Arc::new(std::sync::atomic::AtomicBool::new(true));
        lifecycle_hook(move |_ended| {
            let (trap, first) = (trap.clone(), Arc::clone(&first));
            async move {
                if first.swap(false, Ordering::SeqCst) {
                    trap.0.store(mode, Ordering::SeqCst);
                }
            }
        })
    }

    pub fn hook_panicking(&self) -> LifecycleHook {
        self.armed_by_the_hook(PANIC)
    }

    pub fn hook_failing(&self) -> LifecycleHook {
        self.armed_by_the_hook(ERROR)
    }
}

impl fmt::Debug for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Trap")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Boom {
    #[serde(skip)]
    pub trap: Trap,
}

impl PartialEq for Boom {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Serialize for Boom {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.trap.0.swap(QUIET, Ordering::SeqCst) {
            PANIC => panic!("the fields cannot be serialized"),
            ERROR => Err(serde::ser::Error::custom("the fields cannot be written")),
            _ => serializer.serialize_bool(false),
        }
    }
}

impl Fields for Boom {
    type Update = ();

    fn empty_update() {}

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, _update: ()) -> Result<(), Reason> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct BoomAgent {
    pub lookup: InstanceLookup,
}

struct Worker(Option<BoomAgent>);

impl Node<Boom> for Worker {
    fn name(&self) -> &str {
        "work"
    }

    fn kind(&self) -> &str {
        "programmatic"
    }

    fn subkind(&self) -> &str {
        "worker"
    }

    fn create(&self, _cancel: CancelView, id: InstanceId) -> CreateFuture<'_, Boom> {
        let agent = self.0.clone();
        Box::pin(async move {
            let (instance, mut reader, sender) = Instance::<Boom>::channel(id);
            tokio::spawn(async move {
                if let Some(agent) = agent {
                    let book = agent.lookup.book(id).await.expect("a book");
                    let label = br_agentic_contract::AgentLabel::new("main").expect("label");
                    let entry = br_runner_jobs::NodeEntry::with_inbox(
                        "work",
                        agent.lookup.book_lookup(),
                        label.clone(),
                        br_runner_jobs::mark_gate_stored(),
                    );
                    entry.agent_id(&book, &label).expect("an agent id");
                }
                let _ = reader.next().await;
                sender.finish(Event::Result(()));
            });
            Ok(instance)
        })
    }
}

struct End;

impl Edge<Boom> for End {
    fn name(&self) -> &str {
        "work"
    }

    fn source(&self) -> &str {
        "work"
    }

    fn targets(&self) -> &[Target] {
        const END: &[Target] = &[Target::END];
        END
    }

    fn evaluate(&self, _state: &State<Boom>) -> Result<Vec<Target>, Cause> {
        Ok(vec![Target::END])
    }
}

pub fn boom_graph(agent: Option<BoomAgent>) -> Arc<Graph<Boom>> {
    Arc::new(
        GraphBuilder::<Boom>::new("1")
            .entry("work")
            .node(Arc::new(Worker(agent)))
            .edge(Arc::new(End))
            .build()
            .expect("a graph"),
    )
}
