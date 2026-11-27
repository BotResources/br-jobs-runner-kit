use super::*;
use crate::cluster::fixtures::{Bed, managed};

#[tokio::test(start_paused = true)]
async fn it_is_not_ready_until_the_views_have_synced_and_the_store_is_good() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    let readiness = Readiness::starting();
    assert!(!readiness.is_ready());
    readiness.watch(bed.views.clone());
    readiness.store_ready();
    assert!(!readiness.is_ready());
    bed.until("the views synced", |views| {
        views.not_ready_reason().is_none()
    })
    .await;
    assert_eq!(readiness.not_ready_reason(), None);
}

#[tokio::test(start_paused = true)]
async fn the_store_alone_does_not_make_it_ready_and_synced_views_alone_do_not_either() {
    let bed = Bed::start();
    let readiness = Readiness::starting();
    readiness.watch(bed.views.clone());
    bed.until("the views synced", |views| {
        views.not_ready_reason().is_none()
    })
    .await;
    assert!(
        readiness
            .not_ready_reason()
            .is_some_and(|reason| reason.contains("object store"))
    );
    readiness.store_ready();
    assert!(readiness.is_ready());
}

#[tokio::test(start_paused = true)]
async fn a_zero_resource_cluster_with_synced_views_is_ready() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    let readiness = Readiness::starting();
    readiness.watch(bed.views.clone());
    readiness.store_ready();
    bed.until("the pods and the resources of sbx synced", |views| {
        views.not_ready_reason().is_none() && views.pools("sbx").observed().is_some()
    })
    .await;
    assert!(readiness.is_ready());
}

#[tokio::test(start_paused = true)]
async fn a_missing_definition_is_not_ready_names_the_definition_and_is_ready_again_when_it_is_back()
{
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    let readiness = Readiness::starting();
    readiness.watch(bed.views.clone());
    readiness.store_ready();
    bed.until("synced", |views| views.pools("sbx").observed().is_some())
        .await;
    bed.kube.remove_definition("sbx");
    bed.until("the definition is missing", |views| {
        views.not_ready_reason().is_some()
    })
    .await;
    assert!(
        readiness
            .not_ready_reason()
            .is_some_and(|reason| reason.contains("definition of SandboxPool is absent"))
    );
    bed.kube.resync_watch("pools", "sbx");
    bed.until("synced again", |views| views.not_ready_reason().is_none())
        .await;
    assert!(readiness.is_ready());
}

#[tokio::test(start_paused = true)]
async fn a_watch_that_breaks_after_the_first_sync_does_not_make_it_not_ready() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    let readiness = Readiness::starting();
    readiness.watch(bed.views.clone());
    readiness.store_ready();
    bed.until("synced", |views| views.pools("sbx").observed().is_some())
        .await;
    assert!(readiness.is_ready());
    bed.kube.break_watch("pools", "sbx", "the watch broke");
    bed.kube.break_watch("pods", "sbx", "the watch broke");
    bed.kube.break_watch("namespaces", "", "the watch broke");
    bed.until("unsynced", |views| views.pools("sbx").observed().is_none())
        .await;
    assert!(readiness.is_ready());
}

#[tokio::test(start_paused = true)]
async fn a_panic_while_the_lock_is_held_does_not_take_the_answer_down() {
    let bed = Bed::start();
    let readiness = std::sync::Arc::new(Readiness::starting());
    let poisoner = readiness.clone();
    let panicked = std::thread::spawn(move || {
        let _guard = poisoner.views.lock().unwrap();
        panic!("a panic that poisons the lock");
    })
    .join();
    assert!(panicked.is_err());
    assert!(readiness.views.is_poisoned());
    assert_eq!(
        readiness.not_ready_reason().as_deref(),
        Some("the views are not started")
    );
    readiness.watch(bed.views.clone());
    readiness.store_ready();
    bed.until("the views synced", |views| {
        views.not_ready_reason().is_none()
    })
    .await;
    assert!(readiness.is_ready());
}
