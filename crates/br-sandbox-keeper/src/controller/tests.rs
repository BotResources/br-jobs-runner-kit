mod condition;
mod expiry;
mod run;
mod sizing;

use std::sync::Arc;
use std::time::Duration;

use br_runner_sandbox::Clock;

use super::*;
use crate::cluster::PodEvent;
use crate::fakes::{Call, FakeKube, TokioClock};
use crate::testing;
use crate::time::at;

const NS: &str = "sbx";
const RS: &str = "warm-p";
pub(super) const START: u64 = 1_000_000;

pub(super) struct Bed {
    pub kube: Arc<FakeKube>,
    pub engine: Engine,
    pub leadership: Arc<Leadership>,
    pub metrics: Arc<Metrics>,
    pub readiness: Arc<Readiness>,
}

pub(super) fn bed(leader: bool) -> Bed {
    let kube = Arc::new(FakeKube::default());
    kube.set_namespaces(&[NS]);
    kube.add_replica_set(NS, RS, Some(2));
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(START)));
    let leadership = Leadership::new(clock.clone());
    if leader {
        leadership.renewed(at(START + 100_000_000));
    }
    let metrics = Arc::new(Metrics::new());
    let readiness = Arc::new(Readiness::new([NS.to_owned()]));
    let engine = Engine::new(
        kube.clone(),
        clock,
        leadership.clone(),
        metrics.clone(),
        readiness.clone(),
        Arc::new(testing::config()),
    );
    Bed {
        kube,
        engine,
        leadership,
        metrics,
        readiness,
    }
}

impl Bed {
    pub async fn sync(&mut self) {
        let pods = self.kube.pods_in(NS);
        self.engine.handle(NS, PodEvent::Snapshot(pods)).await;
    }

    pub async fn applied(&mut self, pod: crate::facts::PodFacts) {
        self.engine
            .handle(NS, PodEvent::Applied(Box::new(pod)))
            .await;
    }

    pub async fn pass(&mut self, seconds: u64) {
        for _ in 0..seconds {
            tokio::time::advance(Duration::from_secs(1)).await;
            self.engine.wake().await;
        }
    }

    pub fn replicas(&self) -> Option<i32> {
        self.kube.replica_set(NS, RS).replicas
    }

    pub fn replica_writes(&self) -> Vec<i32> {
        self.kube
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Replicas { replicas, .. } => Some(replicas),
                _ => None,
            })
            .collect()
    }

    pub fn deletes(&self) -> Vec<String> {
        self.kube
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Delete { name, .. } => Some(name),
                _ => None,
            })
            .collect()
    }

    pub fn metrics_text(&self) -> String {
        self.metrics.render(self.leadership.is_leader())
    }

    pub fn has_metric(&self, line: &str) -> bool {
        self.metrics_text().lines().any(|text| text == line)
    }
}

pub(super) fn old_warm(name: &str, age: u64) -> crate::facts::PodFacts {
    crate::facts::fixtures::warm(name, START - age)
}
