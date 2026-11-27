use tokio::sync::mpsc;

use super::{State, Views};
use crate::cluster::{Notice, Watched};

pub(super) fn resource(watched: Watched, namespace: &str, failure: Option<String>) -> Notice {
    Notice::Resource {
        watched,
        namespace: namespace.to_owned(),
        failure,
    }
}

impl State {
    fn managed_views_reason(&self) -> Option<String> {
        self.pods
            .iter()
            .find_map(|(namespace, view)| {
                view.first_sync_reason(&format!("the pods of {namespace}"))
            })
            .or_else(|| {
                self.pools.iter().find_map(|(namespace, view)| {
                    view.first_sync_reason(&format!("the SandboxPool resources of {namespace}"))
                })
            })
    }
}

impl Views {
    pub fn notices(&self) -> mpsc::UnboundedReceiver<Notice> {
        let (sender, receiver) = mpsc::unbounded_channel();
        *self
            .shared
            .notices
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sender);
        receiver
    }

    pub(super) fn notify(&self, notice: Notice) {
        if let Some(sender) = self
            .shared
            .notices
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            let _ = sender.send(notice);
        }
    }

    pub fn not_ready_reason(&self) -> Option<String> {
        let state = self.state();
        for (namespace, view) in &state.pools {
            if view.is_absent() {
                let what = format!("the SandboxPool resources of {namespace}");
                return Some(format!(
                    "the definition of SandboxPool is absent: {}",
                    view.unsynced_reason(&what).unwrap_or_default()
                ));
            }
        }
        if let Some(reason) = state.namespaces.first_sync_reason("the managed namespaces") {
            return Some(reason);
        }
        if let Some(reason) = state.bindings.first_sync_reason("the admission binding") {
            return Some(reason);
        }
        state.managed_views_reason()
    }
}
