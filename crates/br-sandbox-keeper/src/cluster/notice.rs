use crate::facts::PodFacts;

use super::ViewEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Watched {
    Namespaces,
    Bindings,
    Pools,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Notice {
    Pods {
        namespace: String,
        event: Box<ViewEvent<PodFacts>>,
    },
    Resource {
        watched: Watched,
        namespace: String,
        failure: Option<String>,
    },
}

pub(super) fn failure_of<T>(event: &ViewEvent<T>) -> Option<String> {
    match event {
        ViewEvent::Failed { reason } | ViewEvent::Missing { reason } => Some(reason.clone()),
        _ => None,
    }
}
