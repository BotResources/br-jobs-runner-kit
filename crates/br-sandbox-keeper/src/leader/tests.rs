use std::sync::Arc;

use super::*;
use crate::fakes::{Call, FakeKube, TokioClock};
use crate::time::at;

fn lease(holder: Option<&str>, renewed: Option<u64>, duration: u64) -> LeaseFacts {
    LeaseFacts {
        holder: holder.map(str::to_owned),
        acquire_time: Some(at(1)),
        renew_time: renewed.map(at),
        duration: Some(Duration::from_secs(duration)),
        transitions: 4,
        resource_version: "9".to_owned(),
    }
}

fn settings() -> LeaseSettings {
    let config = crate::testing::config();
    config.lease
}

#[test]
fn no_lease_is_created() {
    assert_eq!(
        lease_action(None, "a", at(100), Duration::from_secs(15)),
        LeaseAction::Create
    );
}

#[test]
fn the_holder_renews_its_own_lease() {
    let own = lease(Some("a"), Some(90), 15);
    assert_eq!(
        lease_action(Some(&own), "a", at(100), Duration::from_secs(15)),
        LeaseAction::Renew
    );
}

#[test]
fn a_lease_of_another_replica_is_waited_for_until_its_duration_has_passed() {
    let other = lease(Some("b"), Some(90), 15);
    assert_eq!(
        lease_action(Some(&other), "a", at(104), Duration::from_secs(15)),
        LeaseAction::Wait {
            recheck_at: Some(at(105))
        }
    );
    assert_eq!(
        lease_action(Some(&other), "a", at(105), Duration::from_secs(15)),
        LeaseAction::Take
    );
}

#[test]
fn a_released_or_never_renewed_lease_is_taken() {
    assert_eq!(
        lease_action(
            Some(&lease(None, Some(99), 15)),
            "a",
            at(100),
            Duration::from_secs(15)
        ),
        LeaseAction::Take
    );
    assert_eq!(
        lease_action(
            Some(&lease(Some("b"), None, 15)),
            "a",
            at(100),
            Duration::from_secs(15)
        ),
        LeaseAction::Take
    );
}

#[test]
fn a_lease_without_a_duration_uses_the_own_duration() {
    let mut other = lease(Some("b"), Some(90), 15);
    other.duration = None;
    assert_eq!(
        lease_action(Some(&other), "a", at(100), Duration::from_secs(15)),
        LeaseAction::Wait {
            recheck_at: Some(at(105))
        }
    );
}

mod failures;

struct Replica {
    elector: Elector,
    leadership: Arc<Leadership>,
    metrics: Arc<Metrics>,
}

fn replica(kube: &Arc<FakeKube>, clock: &Arc<TokioClock>, identity: &str) -> Replica {
    let leadership = Leadership::new(clock.clone());
    let metrics = Arc::new(Metrics::new());
    Replica {
        elector: Elector {
            api: kube.clone(),
            clock: clock.clone(),
            leadership: leadership.clone(),
            namespace: "sys".to_owned(),
            identity: identity.to_owned(),
            lease: settings(),
            metrics: metrics.clone(),
        },
        leadership,
        metrics,
    }
}

fn clock() -> Arc<TokioClock> {
    Arc::new(TokioClock::starting_at(at(1000)))
}

#[tokio::test(start_paused = true)]
async fn the_first_replica_creates_the_lease_and_leads() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    assert!(!a.leadership.is_leader());
    a.elector.round().await;
    assert!(a.leadership.is_leader());
    assert_eq!(
        kube.calls(),
        vec![Call::LeaseCreate {
            holder: "a".to_owned()
        }]
    );
    assert_eq!(kube.lease().unwrap().transitions, 0);
}

#[tokio::test(start_paused = true)]
async fn the_second_replica_follows_and_never_writes() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    let b = replica(&kube, &clock, "b");
    a.elector.round().await;
    kube.clear_calls();
    b.elector.round().await;
    assert!(!b.leadership.is_leader());
    assert!(kube.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_leader_that_keeps_renewing_keeps_the_lease() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    let b = replica(&kube, &clock, "b");
    for _ in 0..20 {
        a.elector.round().await;
        b.elector.round().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert!(a.leadership.is_leader());
    assert!(!b.leadership.is_leader());
    assert_eq!(kube.lease().unwrap().holder.as_deref(), Some("a"));
}

#[tokio::test(start_paused = true)]
async fn a_follower_leads_at_most_the_lease_duration_after_the_leader_stops_renewing() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    let b = replica(&kube, &clock, "b");
    a.elector.round().await;
    let renewed_at = clock.now();
    let mut took = None;
    for _ in 0..40 {
        let pause = b.elector.round().await;
        if b.leadership.is_leader() {
            took = Some(clock.now());
            break;
        }
        tokio::time::sleep(pause).await;
    }
    let took = took.expect("the follower takes the lease");
    assert!(crate::time::elapsed(took, renewed_at) <= Duration::from_secs(15));
    assert_eq!(kube.lease().unwrap().holder.as_deref(), Some("b"));
    assert_eq!(kube.lease().unwrap().transitions, 1);
}

#[tokio::test(start_paused = true)]
async fn a_leader_refuses_to_lead_after_the_renew_deadline_without_a_renewal() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    a.elector.round().await;
    kube.fail("get_lease");
    tokio::time::sleep(Duration::from_secs(9)).await;
    a.elector.round().await;
    assert!(a.leadership.is_leader());
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!a.leadership.is_leader());
}

#[tokio::test(start_paused = true)]
async fn a_leader_that_sees_the_lease_of_another_replica_stops_at_once() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    a.elector.round().await;
    let now = clock.now();
    kube.set_lease(Some(LeaseFacts {
        holder: Some("b".to_owned()),
        acquire_time: Some(now),
        renew_time: Some(now),
        duration: Some(Duration::from_secs(15)),
        transitions: 1,
        resource_version: "77".to_owned(),
    }));
    a.elector.round().await;
    assert!(!a.leadership.is_leader());
}

#[tokio::test(start_paused = true)]
async fn a_conflict_on_a_renewal_does_not_end_the_leadership_before_its_deadline() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    a.elector.round().await;
    kube.conflict_next_lease_writes(1);
    tokio::time::sleep(Duration::from_secs(2)).await;
    a.elector.round().await;
    assert!(a.leadership.is_leader());
    a.elector.round().await;
    assert!(a.leadership.is_leader());
}

#[tokio::test(start_paused = true)]
async fn the_gain_of_the_lease_wakes_the_waiter_once() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    let waiter = tokio::spawn({
        let leadership = a.leadership.clone();
        async move { leadership.gained().await }
    });
    tokio::task::yield_now().await;
    a.elector.round().await;
    waiter.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_loop_ends_at_the_shutdown_signal() {
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(a.elector.run(shutdown.clone()));
    tokio::time::sleep(Duration::from_secs(5)).await;
    shutdown.cancel();
    handle.await.unwrap();
}
