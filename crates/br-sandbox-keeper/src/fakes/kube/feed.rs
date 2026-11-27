use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use tokio::sync::mpsc;

use crate::cluster::ViewEvent;

pub(super) type Filter<T> = Arc<dyn Fn(&T) -> bool + Send + Sync>;

struct Opened<T> {
    scope: String,
    filter: Filter<T>,
    sender: mpsc::UnboundedSender<ViewEvent<T>>,
}

pub(super) struct Feed<T> {
    key: fn(&T) -> String,
    items: BTreeMap<(String, String), T>,
    opened: Vec<Opened<T>>,
    opens: Vec<String>,
}

impl<T: Clone + Send + 'static> Feed<T> {
    pub fn new(key: fn(&T) -> String) -> Self {
        Self {
            key,
            items: BTreeMap::new(),
            opened: Vec::new(),
            opens: Vec::new(),
        }
    }

    fn visible(&self, scope: &str, filter: &Filter<T>) -> Vec<T> {
        self.items
            .iter()
            .filter(|((item_scope, _), item)| item_scope == scope && filter(item))
            .map(|(_, item)| item.clone())
            .collect()
    }

    pub fn open(
        &mut self,
        scope: &str,
        filter: Option<Filter<T>>,
    ) -> BoxStream<'static, ViewEvent<T>> {
        let filter = filter.unwrap_or_else(|| Arc::new(|_: &T| true));
        let (sender, receiver) = mpsc::unbounded_channel();
        sender
            .send(ViewEvent::Snapshot(self.visible(scope, &filter)))
            .expect("a fresh channel");
        self.opens.push(scope.to_owned());
        self.opened.push(Opened {
            scope: scope.to_owned(),
            filter,
            sender,
        });
        stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        })
        .boxed()
    }

    pub fn opens(&self, scope: &str) -> usize {
        self.opens.iter().filter(|opened| *opened == scope).count()
    }

    pub fn put(&mut self, scope: &str, item: T) {
        let key = (self.key)(&item);
        self.items
            .insert((scope.to_owned(), key.clone()), item.clone());
        self.opened.retain(|opened| {
            if opened.scope != scope {
                return true;
            }
            let event = if (opened.filter)(&item) {
                ViewEvent::Applied(item.clone())
            } else {
                ViewEvent::Deleted { key: key.clone() }
            };
            opened.sender.send(event).is_ok()
        });
    }

    pub fn remove(&mut self, scope: &str, key: &str) {
        self.items.remove(&(scope.to_owned(), key.to_owned()));
        self.broadcast(scope, |_| ViewEvent::Deleted {
            key: key.to_owned(),
        });
    }

    pub fn fail(&mut self, scope: &str, reason: &str) {
        self.broadcast(scope, |_| ViewEvent::Failed {
            reason: reason.to_owned(),
        });
    }

    pub fn resync(&mut self, scope: &str) {
        let items = self.items.clone();
        self.opened.retain(|opened| {
            if opened.scope != scope {
                return true;
            }
            let visible: Vec<T> = items
                .iter()
                .filter(|((item_scope, _), item)| item_scope == scope && (opened.filter)(item))
                .map(|(_, item)| item.clone())
                .collect();
            opened.sender.send(ViewEvent::Snapshot(visible)).is_ok()
        });
    }

    fn broadcast(&mut self, scope: &str, event: impl Fn(&Opened<T>) -> ViewEvent<T>) {
        self.opened
            .retain(|opened| opened.scope != scope || opened.sender.send(event(opened)).is_ok());
    }
}
