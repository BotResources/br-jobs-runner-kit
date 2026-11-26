use std::sync::Arc;

use br_agentic_harness_contract::{InstanceId, TurnRef};

use crate::error::Error;

use super::state::{BoxRecord, Inner};

pub(super) struct OpenGuard {
    inner: Arc<Inner>,
    instance: InstanceId,
    pod: Option<String>,
    armed: bool,
}

impl OpenGuard {
    pub fn take(inner: &Arc<Inner>, instance: InstanceId) -> Result<Self, Error> {
        if !inner.state().instances.insert(instance) {
            return Err(Error::InstanceOpen(instance));
        }
        Ok(Self {
            inner: inner.clone(),
            instance,
            pod: None,
            armed: true,
        })
    }

    pub fn hold(&mut self, pod: String) {
        self.pod = Some(pod);
    }

    pub fn pod(&self) -> Option<String> {
        self.pod.clone()
    }

    pub fn forget_pod(&mut self) {
        self.pod = None;
    }

    pub fn commit(mut self, pod: String, turn: TurnRef) {
        self.inner.state().boxes.insert(
            pod,
            BoxRecord {
                instance: self.instance,
                turn,
                saved_bytes: None,
                released: false,
            },
        );
        self.armed = false;
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.inner.state().instances.remove(&self.instance);
        let Some(pod) = self.pod.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let cluster = self.inner.env.cluster.clone();
            runtime.spawn(async move {
                let _ = cluster.delete_pod(&pod).await;
            });
        }
    }
}
