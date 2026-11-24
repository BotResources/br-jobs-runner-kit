use std::sync::Arc;

use br_graph_contract::{
    CancelView, Cause, CreateFuture, Edge, Event, Graph, GraphBuilder, Instance, InstanceId, Node,
    State, Target,
};

use crate::fields::{AddLine, Report};
use crate::node::MailNode;

struct MailGraphNode(Arc<MailNode>);

impl Node<Report> for MailGraphNode {
    fn name(&self) -> &str {
        "analyst"
    }

    fn kind(&self) -> &str {
        "programmatic"
    }

    fn subkind(&self) -> &str {
        "mail"
    }

    fn create(&self, _cancel: CancelView, id: InstanceId) -> CreateFuture<'_, Report> {
        let node = Arc::clone(&self.0);
        Box::pin(async move {
            let (instance, mut reader, sender) = Instance::<Report>::channel(id);
            tokio::spawn(async move {
                let outcome = node
                    .work(id)
                    .await
                    .unwrap_or_else(|error| format!("failed: {error}"));
                node.observed
                    .outcomes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(outcome.clone());
                let _ = reader.next().await;
                sender.finish(Event::Result(AddLine(outcome)));
            });
            Ok(instance)
        })
    }
}

struct ToEnd;

impl Edge<Report> for ToEnd {
    fn name(&self) -> &str {
        "analyst"
    }

    fn source(&self) -> &str {
        "analyst"
    }

    fn targets(&self) -> &[Target] {
        &[Target::END]
    }

    fn evaluate(&self, _state: &State<Report>) -> Result<Vec<Target>, Cause> {
        Ok(vec![Target::END])
    }
}

pub fn mail_graph(
    node: Arc<MailNode>,
) -> Result<Arc<Graph<Report>>, Box<dyn std::error::Error + Send + Sync>> {
    let graph = GraphBuilder::<Report>::new("1")
        .entry("analyst")
        .node(Arc::new(MailGraphNode(node)))
        .edge(Arc::new(ToEnd))
        .build()?;
    Ok(Arc::new(graph))
}
