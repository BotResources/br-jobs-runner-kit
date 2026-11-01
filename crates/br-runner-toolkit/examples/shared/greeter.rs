use std::sync::Arc;

use br_graph_contract::{
    CancelView, Cause, CreateFuture, Edge, Event, Fields, Graph, GraphBuilder, Instance,
    InstanceId, Node, Reason, State, Target,
};
use br_runner_toolkit::{RunRefusal, RunStart, StartedRun, run_start};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Greetings {
    pub lines: Vec<String>,
}

pub struct AddLine(pub String);

impl Fields for Greetings {
    type Update = AddLine;

    fn empty_update() -> AddLine {
        AddLine(String::new())
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: AddLine) -> Result<(), Reason> {
        if !update.0.is_empty() {
            self.lines.push(update.0);
        }
        Ok(())
    }
}

pub type Setup = Arc<dyn Fn(InstanceId) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

pub struct Greeter {
    pub setup: Option<Setup>,
}

impl Node<Greetings> for Greeter {
    fn name(&self) -> &str {
        "greeter"
    }

    fn kind(&self) -> &str {
        "programmatic"
    }

    fn subkind(&self) -> &str {
        "greeter"
    }

    fn create(&self, _cancel: CancelView, id: InstanceId) -> CreateFuture<'_, Greetings> {
        let setup = self.setup.clone();
        Box::pin(async move {
            let (instance, mut reader, sender) = Instance::<Greetings>::channel(id);
            tokio::spawn(async move {
                if let Some(setup) = setup {
                    setup(id).await;
                }
                let _ = reader.next().await;
                sender.finish(Event::Result(AddLine("hello".to_owned())));
            });
            Ok(instance)
        })
    }
}

struct ToEnd;

impl Edge<Greetings> for ToEnd {
    fn name(&self) -> &str {
        "greeter"
    }

    fn source(&self) -> &str {
        "greeter"
    }

    fn targets(&self) -> &[Target] {
        &[Target::END]
    }

    fn evaluate(&self, _state: &State<Greetings>) -> Result<Vec<Target>, Cause> {
        Ok(vec![Target::END])
    }
}

pub fn graph(
    setup: Option<Setup>,
) -> Result<Arc<Graph<Greetings>>, Box<dyn std::error::Error + Send + Sync>> {
    let graph = GraphBuilder::<Greetings>::new("1")
        .entry("greeter")
        .node(Arc::new(Greeter { setup }))
        .edge(Arc::new(ToEnd))
        .build()?;
    Ok(Arc::new(graph))
}

pub fn starter() -> Arc<dyn RunStart<Greetings>> {
    run_start(|input| async move {
        Ok::<_, RunRefusal>(StartedRun::new(
            input.restored.unwrap_or_default(),
            vec!["greeter".to_owned()],
        ))
    })
}
