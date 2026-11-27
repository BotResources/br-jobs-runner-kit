use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use br_runner_sandbox::PodKeys;
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use kube::api::{ListParams, WatchEvent, WatchParams};
use kube::{Api, Resource, ResourceExt};
use serde::de::DeserializeOwned;

use crate::cluster::ViewEvent;

const WATCH_RETRY: Duration = Duration::from_secs(1);

pub(super) fn pod_selector(keys: &PodKeys) -> String {
    format!(
        "{} in ({},{})",
        keys.pool(),
        br_runner_sandbox::POOL_WARM,
        br_runner_sandbox::POOL_CLAIMED
    )
}

type Events<K> = BoxStream<'static, kube::Result<WatchEvent<K>>>;

enum Phase<K> {
    List,
    Watch { version: String, events: Events<K> },
}

pub(super) type Convert<K, T> = Arc<dyn Fn(&K) -> Option<T> + Send + Sync>;

struct Watch<K, T> {
    api: Api<K>,
    list: ListParams,
    watch: WatchParams,
    what: &'static str,
    convert: Convert<K, T>,
}

fn non_empty(candidate: Option<&String>, previous: String) -> String {
    candidate
        .filter(|text| !text.is_empty())
        .cloned()
        .unwrap_or(previous)
}

fn failure<T>(what: &str, error: impl std::fmt::Display) -> ViewEvent<T> {
    ViewEvent::Failed {
        reason: format!("{what}: {error}"),
    }
}

fn list_failure<T>(what: &str, error: &kube::Error) -> ViewEvent<T> {
    match error {
        kube::Error::Api(status) if status.is_not_found() => ViewEvent::Missing {
            reason: format!(
                "{what}: the API server does not serve it ({})",
                status.message
            ),
        },
        other => failure(what, other),
    }
}

impl<K, T> Watch<K, T>
where
    K: Resource<DynamicType = ()> + Clone + DeserializeOwned + Debug + Send + 'static,
{
    async fn open(&self, version: &str) -> kube::Result<Events<K>> {
        self.api
            .watch(&self.watch, version)
            .await
            .map(StreamExt::boxed)
    }

    async fn pause_then_relist(&self, failed: ViewEvent<T>) -> (Vec<ViewEvent<T>>, Phase<K>) {
        tokio::time::sleep(WATCH_RETRY).await;
        (vec![failed], Phase::List)
    }

    async fn relist(&self) -> (Vec<ViewEvent<T>>, Phase<K>) {
        let list = match self.api.list(&self.list).await {
            Ok(list) => list,
            Err(error) => {
                let what = format!("the {} list failed", self.what);
                return self.pause_then_relist(list_failure(&what, &error)).await;
            }
        };
        let version = list.metadata.resource_version.clone().unwrap_or_default();
        let items = list.items.iter().filter_map(&*self.convert).collect();
        let snapshot = ViewEvent::Snapshot(items);
        match self.open(&version).await {
            Ok(events) => (vec![snapshot], Phase::Watch { version, events }),
            Err(error) => {
                let what = format!("the {} watch could not be opened", self.what);
                let (failed, phase) = self.pause_then_relist(failure(&what, error)).await;
                (vec![snapshot].into_iter().chain(failed).collect(), phase)
            }
        }
    }

    async fn step(&self, phase: Phase<K>) -> (Vec<ViewEvent<T>>, Phase<K>) {
        let Phase::Watch {
            version,
            mut events,
        } = phase
        else {
            return self.relist().await;
        };
        match events.next().await {
            Some(Ok(WatchEvent::Added(object) | WatchEvent::Modified(object))) => {
                let version = non_empty(object.meta().resource_version.as_ref(), version);
                let applied = (self.convert)(&object).map(ViewEvent::Applied);
                (
                    applied.into_iter().collect(),
                    Phase::Watch { version, events },
                )
            }
            Some(Ok(WatchEvent::Deleted(object))) => {
                let version = non_empty(object.meta().resource_version.as_ref(), version);
                let key = object.name_any();
                (
                    vec![ViewEvent::Deleted { key }],
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
                    "the {} watch sent an error event ({} {})",
                    self.what, status.code, status.reason
                );
                self.pause_then_relist(failure(&what, &status.message))
                    .await
            }
            Some(Err(error)) => {
                let what = format!("the {} watch broke", self.what);
                self.pause_then_relist(failure(&what, error)).await
            }
            None => match self.open(&version).await {
                Ok(events) => (Vec::new(), Phase::Watch { version, events }),
                Err(error) => {
                    let what = format!("the {} watch could not be reopened", self.what);
                    self.pause_then_relist(failure(&what, error)).await
                }
            },
        }
    }
}

pub(super) fn watch_stream<K, T>(
    api: Api<K>,
    list: ListParams,
    watch: WatchParams,
    what: &'static str,
    convert: Convert<K, T>,
) -> BoxStream<'static, ViewEvent<T>>
where
    K: Resource<DynamicType = ()> + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    T: Send + 'static,
{
    let watch = Watch {
        api,
        list,
        watch,
        what,
        convert,
    };
    stream::unfold((watch, Phase::List), |(watch, phase)| async move {
        let (events, phase) = watch.step(phase).await;
        Some((stream::iter(events), (watch, phase)))
    })
    .flatten()
    .boxed()
}
