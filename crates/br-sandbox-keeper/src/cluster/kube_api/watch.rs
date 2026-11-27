use std::time::Duration;

use br_runner_sandbox::PodKeys;
use futures::StreamExt;
use futures::stream::BoxStream;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::{ListParams, WatchEvent, WatchParams};

use crate::convert::pod_facts;

use super::PodEvent;

const WATCH_RETRY: Duration = Duration::from_secs(1);

pub(super) fn pod_selector(keys: &PodKeys) -> String {
    format!(
        "{} in ({},{})",
        keys.pool(),
        br_runner_sandbox::POOL_WARM,
        br_runner_sandbox::POOL_CLAIMED
    )
}

pub(super) enum Phase {
    List,
    Watch {
        version: String,
        events: BoxStream<'static, kube::Result<WatchEvent<Pod>>>,
    },
}

pub(super) struct Watch {
    pub(super) api: Api<Pod>,
    pub(super) list: ListParams,
    pub(super) watch: WatchParams,
    pub(super) keys: PodKeys,
}

fn non_empty(candidate: Option<&String>, previous: String) -> String {
    candidate
        .filter(|text| !text.is_empty())
        .cloned()
        .unwrap_or(previous)
}

fn failure(what: &str, error: impl std::fmt::Display) -> PodEvent {
    PodEvent::WatchFailed {
        reason: format!("{what}: {error}"),
    }
}

impl Watch {
    async fn open(
        &self,
        version: &str,
    ) -> kube::Result<BoxStream<'static, kube::Result<WatchEvent<Pod>>>> {
        self.api
            .watch(&self.watch, version)
            .await
            .map(StreamExt::boxed)
    }

    async fn pause_then_relist(&self, failed: PodEvent) -> (Vec<PodEvent>, Phase) {
        tokio::time::sleep(WATCH_RETRY).await;
        (vec![failed], Phase::List)
    }

    async fn relist(&self) -> (Vec<PodEvent>, Phase) {
        let list = match self.api.list(&self.list).await {
            Ok(list) => list,
            Err(error) => {
                return self
                    .pause_then_relist(failure("the pod list failed", error))
                    .await;
            }
        };
        let version = list.metadata.resource_version.clone().unwrap_or_default();
        let facts = list
            .items
            .iter()
            .filter_map(|pod| pod_facts(pod, &self.keys))
            .collect();
        let snapshot = PodEvent::Snapshot(facts);
        match self.open(&version).await {
            Ok(events) => (vec![snapshot], Phase::Watch { version, events }),
            Err(error) => {
                let (failed, phase) = self
                    .pause_then_relist(failure("the pod watch could not be opened", error))
                    .await;
                (vec![snapshot].into_iter().chain(failed).collect(), phase)
            }
        }
    }

    pub(super) async fn step(&self, phase: Phase) -> (Vec<PodEvent>, Phase) {
        let Phase::Watch {
            version,
            mut events,
        } = phase
        else {
            return self.relist().await;
        };
        match events.next().await {
            Some(Ok(WatchEvent::Added(pod) | WatchEvent::Modified(pod))) => {
                let version = non_empty(pod.metadata.resource_version.as_ref(), version);
                let applied =
                    pod_facts(&pod, &self.keys).map(|facts| PodEvent::Applied(Box::new(facts)));
                (
                    applied.into_iter().collect(),
                    Phase::Watch { version, events },
                )
            }
            Some(Ok(WatchEvent::Deleted(pod))) => {
                let version = non_empty(pod.metadata.resource_version.as_ref(), version);
                let name = pod.metadata.name.clone().unwrap_or_default();
                (
                    vec![PodEvent::Deleted { name }],
                    Phase::Watch { version, events },
                )
            }
            Some(Ok(WatchEvent::Bookmark(bookmark))) => (
                Vec::new(),
                Phase::Watch {
                    version: bookmark.metadata.resource_version,
                    events,
                },
            ),
            Some(Ok(WatchEvent::Error(status))) => {
                let what = format!(
                    "the pod watch sent an error event ({} {})",
                    status.code, status.reason
                );
                self.pause_then_relist(failure(&what, &status.message))
                    .await
            }
            Some(Err(error)) => {
                self.pause_then_relist(failure("the pod watch broke", error))
                    .await
            }
            None => match self.open(&version).await {
                Ok(events) => (Vec::new(), Phase::Watch { version, events }),
                Err(error) => {
                    self.pause_then_relist(failure("the pod watch could not be reopened", error))
                        .await
                }
            },
        }
    }
}
