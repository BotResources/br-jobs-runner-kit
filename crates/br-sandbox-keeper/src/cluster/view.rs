use std::collections::BTreeMap;

use crate::facts::PodFacts;

use super::PodEvent;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ViewEvent<T> {
    Snapshot(Vec<T>),
    Applied(T),
    Deleted { key: String },
    Failed { reason: String },
}

impl From<PodEvent> for ViewEvent<PodFacts> {
    fn from(event: PodEvent) -> Self {
        match event {
            PodEvent::Snapshot(pods) => ViewEvent::Snapshot(pods),
            PodEvent::Applied(pod) => ViewEvent::Applied(*pod),
            PodEvent::Deleted { name } => ViewEvent::Deleted { key: name },
            PodEvent::WatchFailed { reason } => ViewEvent::Failed { reason },
        }
    }
}

impl From<ViewEvent<PodFacts>> for PodEvent {
    fn from(event: ViewEvent<PodFacts>) -> Self {
        match event {
            ViewEvent::Snapshot(pods) => PodEvent::Snapshot(pods),
            ViewEvent::Applied(pod) => PodEvent::Applied(Box::new(pod)),
            ViewEvent::Deleted { key } => PodEvent::Deleted { name: key },
            ViewEvent::Failed { reason } => PodEvent::WatchFailed { reason },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Read<T> {
    Observed(T),
    NotObserved(String),
}

impl<T> Read<T> {
    pub fn not_observed(reason: impl Into<String>) -> Self {
        Read::NotObserved(reason.into())
    }

    pub fn observed(self) -> Option<T> {
        match self {
            Read::Observed(value) => Some(value),
            Read::NotObserved(_) => None,
        }
    }

    pub fn map<U>(self, change: impl FnOnce(T) -> U) -> Read<U> {
        match self {
            Read::Observed(value) => Read::Observed(change(value)),
            Read::NotObserved(reason) => Read::NotObserved(reason),
        }
    }
}

#[derive(Debug, Clone)]
pub struct View<T> {
    key: fn(&T) -> String,
    items: BTreeMap<String, T>,
    synced: bool,
    failure: Option<String>,
}

impl<T: Clone> View<T> {
    pub fn new(key: fn(&T) -> String) -> Self {
        Self {
            key,
            items: BTreeMap::new(),
            synced: false,
            failure: None,
        }
    }

    pub fn apply(&mut self, event: ViewEvent<T>) {
        match event {
            ViewEvent::Snapshot(items) => {
                self.items = items
                    .into_iter()
                    .map(|item| ((self.key)(&item), item))
                    .collect();
                self.synced = true;
                self.failure = None;
            }
            ViewEvent::Applied(item) => {
                self.items.insert((self.key)(&item), item);
            }
            ViewEvent::Deleted { key } => {
                self.items.remove(&key);
            }
            ViewEvent::Failed { reason } => {
                self.synced = false;
                self.failure = Some(reason);
            }
        }
    }

    pub fn is_synced(&self) -> bool {
        self.synced
    }

    pub fn read(&self, what: &str) -> Read<Vec<T>> {
        if self.synced {
            Read::Observed(self.items.values().cloned().collect())
        } else {
            Read::NotObserved(self.not_synced(what))
        }
    }

    pub fn read_one(&self, what: &str, key: &str) -> Read<Option<T>> {
        if self.synced {
            Read::Observed(self.items.get(key).cloned())
        } else {
            Read::NotObserved(self.not_synced(what))
        }
    }

    fn not_synced(&self, what: &str) -> String {
        match &self.failure {
            Some(reason) => format!("the view of {what} is not synced: {reason}"),
            None => format!("the view of {what} has not done its first list"),
        }
    }
}

#[cfg(test)]
mod tests;
