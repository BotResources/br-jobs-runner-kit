use std::time::Duration;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::{ListParams, WatchEvent, WatchParams};

use super::kube_convert::pod_view;
use super::selector::LabelSelector;
use super::views::PodEvent;

const RETRY_PAUSE: Duration = Duration::from_secs(1);

type Events = BoxStream<'static, kube::Result<WatchEvent<Pod>>>;

enum Phase {
    List,
    Watch { version: String, events: Events },
}

struct Watch {
    api: Api<Pod>,
    list: ListParams,
    watch: WatchParams,
}

impl Watch {
    async fn open(&self, version: &str) -> Option<Events> {
        self.api
            .watch(&self.watch, version)
            .await
            .ok()
            .map(StreamExt::boxed)
    }

    async fn step(&self, phase: Phase) -> (Vec<PodEvent>, Phase) {
        match phase {
            Phase::List => self.relist().await,
            Phase::Watch {
                version,
                mut events,
            } => match events.next().await {
                Some(Ok(WatchEvent::Added(pod) | WatchEvent::Modified(pod))) => {
                    let view = pod_view(&pod);
                    let version = non_empty(&view.resource_version, version);
                    (
                        vec![PodEvent::Applied(view)],
                        Phase::Watch { version, events },
                    )
                }
                Some(Ok(WatchEvent::Deleted(pod))) => {
                    let view = pod_view(&pod);
                    let version = non_empty(&view.resource_version, version);
                    (
                        vec![PodEvent::Deleted(view.name)],
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
                Some(Ok(WatchEvent::Error(_)) | Err(_)) => self.pause_then_relist().await,
                None => match self.open(&version).await {
                    Some(events) => (Vec::new(), Phase::Watch { version, events }),
                    None => self.pause_then_relist().await,
                },
            },
        }
    }

    async fn pause_then_relist(&self) -> (Vec<PodEvent>, Phase) {
        tokio::time::sleep(RETRY_PAUSE).await;
        (Vec::new(), Phase::List)
    }

    async fn relist(&self) -> (Vec<PodEvent>, Phase) {
        let Ok(list) = self.api.list(&self.list).await else {
            return self.pause_then_relist().await;
        };
        let version = list.metadata.resource_version.clone().unwrap_or_default();
        let snapshot = PodEvent::Snapshot(list.items.iter().map(pod_view).collect());
        match self.open(&version).await {
            Some(events) => (vec![snapshot], Phase::Watch { version, events }),
            None => {
                tokio::time::sleep(RETRY_PAUSE).await;
                (vec![snapshot], Phase::List)
            }
        }
    }
}

fn non_empty(candidate: &str, previous: String) -> String {
    if candidate.is_empty() {
        previous
    } else {
        candidate.to_owned()
    }
}

pub(super) fn watch_pods(api: Api<Pod>, selector: &LabelSelector) -> BoxStream<'static, PodEvent> {
    let labels = selector.to_string();
    let watch = Watch {
        api,
        list: ListParams::default().labels(&labels),
        watch: WatchParams::default().labels(&labels),
    };
    stream::unfold((watch, Phase::List), |(watch, phase)| async move {
        let (events, phase) = watch.step(phase).await;
        Some((stream::iter(events), (watch, phase)))
    })
    .flatten()
    .boxed()
}
