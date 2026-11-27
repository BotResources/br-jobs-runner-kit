mod bed;
mod bound;
mod condition;
mod expiry;
mod faults;
mod fence;
mod lifecycle;
mod metrics;
mod refusals;
mod restart;
mod rollout;
mod run;
mod served;
mod sizing;
mod status;

use std::sync::Arc;
use std::time::Duration;

use br_runner_sandbox::Clock;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::cluster::fixtures::{
    SELECTOR, binding, deny_all, managed, owned_replica_set, quota, template_json, template_map,
};
use crate::cluster::{Notice, PoolApi};
use crate::convert::replica_set_facts;
use crate::fakes::{Call, FakeKube, TokioClock};
use crate::resource::{PoolSize, SandboxPool, SandboxPoolSpec, SizeBounds};
use crate::testing;
use crate::time::at;

pub(super) const NS: &str = "sbx";
pub(super) const RS: &str = "warm-sbx";
pub(super) const START: u64 = 1_000_000;

pub(super) struct Bed {
    pub kube: Arc<FakeKube>,
    pub engine: Engine,
    pub notices: UnboundedReceiver<Notice>,
    pub leadership: Arc<Leadership>,
    pub metrics: Arc<Metrics>,
}

pub(super) fn spec() -> SandboxPoolSpec {
    SandboxPoolSpec::new(
        "registry.example/agent:1",
        PoolSize::new(
            SizeBounds::new("500m", "2"),
            SizeBounds::new("1Gi", "4Gi"),
            SizeBounds::new("1Gi", "8Gi"),
        ),
        1,
        20,
    )
}

pub(super) fn resource(name: &str, uid: &str, created: u64, spec: &SandboxPoolSpec) -> SandboxPool {
    SandboxPool::new(
        ObjectMeta {
            name: Some(name.to_owned()),
            uid: Some(uid.to_owned()),
            generation: Some(1),
            creation_timestamp: Some(Time(
                k8s_openapi::jiff::Timestamp::from_second(i64::try_from(created).unwrap()).unwrap(),
            )),
            ..ObjectMeta::default()
        },
        spec,
    )
    .unwrap()
}

pub(super) fn edited(spec: &SandboxPoolSpec, generation: i64) -> SandboxPool {
    let mut pool = resource("main", "uid-1", START - 100, spec);
    pool.metadata.generation = Some(generation);
    pool
}

pub(super) fn main_pool() -> SandboxPool {
    resource("main", "uid-1", START - 100, &spec())
}

pub(super) fn bare(leader: bool) -> Bed {
    bare_with(leader, testing::config())
}

pub(super) fn bare_with(leader: bool, config: crate::config::KeeperConfig) -> Bed {
    let kube = Arc::new(FakeKube::default());
    kube.put_namespace(managed(NS));
    kube.put_binding(binding());
    kube.put_quota(NS, quota());
    kube.put_network_policies(NS, vec![deny_all()]);
    kube.put_template(NS, template_map(&template_json(NS)));
    over(kube, leader, config)
}

pub(super) fn over(kube: Arc<FakeKube>, leader: bool, config: crate::config::KeeperConfig) -> Bed {
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(START)));
    let leadership = Leadership::new(clock.clone());
    if leader {
        leadership.renewed(at(START + 100_000_000));
    }
    let metrics = Arc::new(Metrics::new());
    let views = Views::new();
    let notices = views.notices();
    let api: Arc<dyn PoolApi> = kube.clone();
    tokio::spawn(
        views
            .clone()
            .run(api.clone(), SELECTOR.to_owned(), CancellationToken::new()),
    );
    let engine = Engine::new(
        api,
        views,
        clock,
        leadership.clone(),
        metrics.clone(),
        Arc::new(config),
    );
    Bed {
        kube,
        engine,
        notices,
        leadership,
        metrics,
    }
}

pub(super) fn bed(leader: bool) -> Bed {
    let bed = bare(leader);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.put_replica_set(owned_replica_set(NS, 2));
    bed
}

impl Bed {
    pub async fn settle(&mut self) {
        loop {
            for _ in 0..32 {
                tokio::task::yield_now().await;
            }
            let mut delivered = false;
            while let Ok(notice) = self.notices.try_recv() {
                delivered = true;
                self.engine.notice(notice).await;
            }
            if !delivered {
                return;
            }
        }
    }

    pub async fn sync(&mut self) {
        self.settle().await;
    }

    pub async fn pass(&mut self, seconds: u64) {
        for _ in 0..seconds {
            tokio::time::advance(Duration::from_secs(1)).await;
            self.engine.wake().await;
            self.settle().await;
        }
    }
}

pub(super) fn old_warm(name: &str, age: u64) -> crate::facts::PodFacts {
    crate::facts::fixtures::warm(name, START - age)
}
