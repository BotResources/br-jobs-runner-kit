use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use futures::stream::{BoxStream, SelectAll};
use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::core::v1::Namespace;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::facts::PodFacts;
use crate::pool::ADMISSION_BINDING_NAME;
use crate::resource::SandboxPool;

use super::notice::failure_of;
use super::{Notice, PoolApi, Read, View, ViewEvent, Watched};

enum Feed {
    Namespaces(Box<ViewEvent<Namespace>>),
    Bindings(Box<ViewEvent<ValidatingAdmissionPolicyBinding>>),
    Pods(String, Box<ViewEvent<PodFacts>>),
    Pools(String, Box<ViewEvent<SandboxPool>>),
}

struct State {
    namespaces: View<Namespace>,
    bindings: View<ValidatingAdmissionPolicyBinding>,
    pods: BTreeMap<String, View<PodFacts>>,
    pools: BTreeMap<String, View<SandboxPool>>,
}

struct Shared {
    state: Mutex<State>,
    changes: watch::Sender<u64>,
    notices: Mutex<Option<mpsc::UnboundedSender<Notice>>>,
}

#[derive(Clone)]
pub struct Views {
    shared: Arc<Shared>,
}

fn name_of_namespace(namespace: &Namespace) -> String {
    namespace.metadata.name.clone().unwrap_or_default()
}

fn name_of_binding(binding: &ValidatingAdmissionPolicyBinding) -> String {
    binding.metadata.name.clone().unwrap_or_default()
}

fn name_of_pod(pod: &PodFacts) -> String {
    pod.name.clone()
}

fn name_of_pool(pool: &SandboxPool) -> String {
    pool.metadata.name.clone().unwrap_or_default()
}

impl Default for Views {
    fn default() -> Self {
        Self::new()
    }
}

impl Views {
    pub fn new() -> Self {
        let (changes, _) = watch::channel(0);
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    namespaces: View::new(name_of_namespace),
                    bindings: View::new(name_of_binding),
                    pods: BTreeMap::new(),
                    pools: BTreeMap::new(),
                }),
                changes,
                notices: Mutex::new(None),
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.shared.changes.subscribe()
    }

    pub fn managed_namespaces(&self) -> Read<Vec<Namespace>> {
        self.state().namespaces.read("the managed namespaces")
    }

    pub fn namespace(&self, name: &str) -> Read<Option<Namespace>> {
        self.state()
            .namespaces
            .read_one("the managed namespaces", name)
    }

    pub fn binding(&self) -> Read<Option<ValidatingAdmissionPolicyBinding>> {
        self.state()
            .bindings
            .read_one("the admission binding", ADMISSION_BINDING_NAME)
    }

    pub fn pods(&self, namespace: &str) -> Read<Vec<PodFacts>> {
        let what = format!("the pods of {namespace}");
        match self.state().pods.get(namespace) {
            Some(view) => view.read(&what),
            None => Read::not_observed(format!("{what} is not watched")),
        }
    }

    pub fn pools(&self, namespace: &str) -> Read<Vec<SandboxPool>> {
        let what = format!("the SandboxPool resources of {namespace}");
        match self.state().pools.get(namespace) {
            Some(view) => view.read(&what),
            None => Read::not_observed(format!("{what} is not watched")),
        }
    }

    fn apply(&self, feed: Feed) {
        let notice = {
            let mut state = self.state();
            match feed {
                Feed::Namespaces(event) => {
                    let failure = failure_of(&event);
                    state.namespaces.apply(*event);
                    Some(signals::resource(Watched::Namespaces, "", failure))
                }
                Feed::Bindings(event) => {
                    let failure = failure_of(&event);
                    state.bindings.apply(*event);
                    Some(signals::resource(Watched::Bindings, "", failure))
                }
                Feed::Pods(namespace, event) => state.pods.get_mut(&namespace).map(|view| {
                    view.apply((*event).clone());
                    Notice::Pods { namespace, event }
                }),
                Feed::Pools(namespace, event) => {
                    let failure = failure_of(&event);
                    state.pools.get_mut(&namespace).map(|view| {
                        view.apply(*event);
                        signals::resource(Watched::Pools, &namespace, failure)
                    })
                }
            }
        };
        self.shared.changes.send_modify(|version| *version += 1);
        if let Some(notice) = notice {
            self.notify(notice);
        }
    }

    fn managed(&self) -> Option<BTreeSet<String>> {
        self.managed_namespaces().observed().map(|namespaces| {
            namespaces
                .iter()
                .map(name_of_namespace)
                .filter(|name| !name.is_empty())
                .collect()
        })
    }

    fn follow(
        &self,
        api: &Arc<dyn PoolApi>,
        feeds: &mut SelectAll<BoxStream<'static, Feed>>,
        watched: &mut HashMap<String, CancellationToken>,
    ) {
        let Some(managed) = self.managed() else {
            return;
        };
        let gone: Vec<String> = watched
            .keys()
            .filter(|namespace| !managed.contains(*namespace))
            .cloned()
            .collect();
        for namespace in gone {
            if let Some(token) = watched.remove(&namespace) {
                token.cancel();
            }
            let mut state = self.state();
            state.pods.remove(&namespace);
            state.pools.remove(&namespace);
        }
        for namespace in managed {
            if watched.contains_key(&namespace) {
                continue;
            }
            let token = CancellationToken::new();
            {
                let mut state = self.state();
                state.pods.insert(namespace.clone(), View::new(name_of_pod));
                state
                    .pools
                    .insert(namespace.clone(), View::new(name_of_pool));
            }
            let pods = api.watch_pods(&namespace);
            let tagged = namespace.clone();
            feeds.push(
                pods.map(move |event| Feed::Pods(tagged.clone(), Box::new(event.into())))
                    .take_until(token.clone().cancelled_owned())
                    .boxed(),
            );
            let tagged = namespace.clone();
            feeds.push(
                api.watch_pools(&namespace)
                    .map(move |event| Feed::Pools(tagged.clone(), Box::new(event)))
                    .take_until(token.clone().cancelled_owned())
                    .boxed(),
            );
            watched.insert(namespace, token);
        }
    }

    pub async fn run(self, api: Arc<dyn PoolApi>, selector: String, shutdown: CancellationToken) {
        let mut feeds: SelectAll<BoxStream<'static, Feed>> = SelectAll::new();
        feeds.push(
            api.watch_namespaces(&selector)
                .map(|event| Feed::Namespaces(Box::new(event)))
                .boxed(),
        );
        feeds.push(
            api.watch_bindings()
                .map(|event| Feed::Bindings(Box::new(event)))
                .boxed(),
        );
        let mut watched: HashMap<String, CancellationToken> = HashMap::new();
        loop {
            let feed = tokio::select! {
                () = shutdown.cancelled() => break,
                feed = feeds.next() => feed,
            };
            let Some(feed) = feed else { break };
            self.apply(feed);
            self.follow(&api, &mut feeds, &mut watched);
        }
        for token in watched.values() {
            token.cancel();
        }
    }
}

mod signals;
#[cfg(test)]
mod tests;
