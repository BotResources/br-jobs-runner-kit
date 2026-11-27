use super::*;
use crate::cluster::fixtures::{Bed, binding, managed, namespace, pool};
use crate::facts::PodFacts;
use crate::facts::PoolState;
use crate::time::at;

fn pod(namespace: &str, name: &str) -> PodFacts {
    PodFacts::new(
        namespace,
        name,
        format!("uid-{name}"),
        "1",
        PoolState::Warm,
        at(1),
    )
}

#[tokio::test]
async fn no_view_is_read_before_its_first_list_has_arrived() {
    let views = Views::new();
    assert!(matches!(views.managed_namespaces(), Read::NotObserved(_)));
    assert!(matches!(views.namespace("sbx"), Read::NotObserved(_)));
    assert!(matches!(views.binding(), Read::NotObserved(_)));
    assert!(
        matches!(views.pods("sbx"), Read::NotObserved(reason) if reason.contains("not watched"))
    );
    assert!(
        matches!(views.pools("sbx"), Read::NotObserved(reason) if reason.contains("not watched"))
    );
}

#[tokio::test]
async fn one_watch_of_pods_and_one_of_pools_is_held_for_each_managed_namespace_and_for_no_other() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx-a"));
    bed.kube.put_namespace(managed("sbx-b"));
    bed.kube
        .put_namespace(namespace("product", &[("team", "x")]));
    bed.until("both pod views synced", |views| {
        views.pods("sbx-a").observed().is_some() && views.pods("sbx-b").observed().is_some()
    })
    .await;
    bed.until("both pool views synced", |views| {
        views.pools("sbx-a").observed().is_some() && views.pools("sbx-b").observed().is_some()
    })
    .await;
    assert!(matches!(bed.views.pods("product"), Read::NotObserved(_)));
    assert_eq!(bed.kube.watch_opens("pools", "sbx-a"), 1);
    assert_eq!(bed.kube.watch_opens("pools", "sbx-b"), 1);
    assert_eq!(bed.kube.watch_opens("pools", "product"), 0);
    let names: Vec<String> = bed
        .views
        .managed_namespaces()
        .observed()
        .unwrap()
        .iter()
        .map(|found| found.metadata.name.clone().unwrap())
        .collect();
    assert_eq!(names, ["sbx-a", "sbx-b"]);
    let opened = bed.kube.reads();
    assert!(opened.contains(&"watch_namespaces"));
    assert!(opened.contains(&"watch_bindings"));
}

#[tokio::test]
async fn the_cluster_wide_watches_are_the_namespaces_and_the_bindings_only() {
    let bed = Bed::start();
    bed.until("namespaces and binding synced", |views| {
        views.managed_namespaces().observed().is_some() && views.binding().observed().is_some()
    })
    .await;
    let cluster_wide: Vec<&str> = bed
        .kube
        .reads()
        .into_iter()
        .filter(|call| call.starts_with("watch_"))
        .collect();
    assert_eq!(cluster_wide, ["watch_namespaces", "watch_bindings"]);
}

#[tokio::test]
async fn the_pods_and_the_pools_of_a_namespace_are_folded_into_its_views() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    bed.kube.put_pod(pod("sbx", "w0"));
    bed.kube.put_pool("sbx", pool("main", "uid-1"));
    bed.until("pod and pool seen", |views| {
        views
            .pods("sbx")
            .observed()
            .is_some_and(|pods| pods.len() == 1)
            && views
                .pools("sbx")
                .observed()
                .is_some_and(|pools| pools.len() == 1)
    })
    .await;
    bed.kube.put_pod(pod("sbx", "w1"));
    bed.kube.remove_pool("sbx", "main");
    bed.until("change folded", |views| {
        views
            .pods("sbx")
            .observed()
            .is_some_and(|pods| pods.len() == 2)
            && views
                .pools("sbx")
                .observed()
                .is_some_and(|pools| pools.is_empty())
    })
    .await;
}

#[tokio::test]
async fn a_failed_watch_unsyncs_that_view_only_until_the_next_list() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    bed.kube.put_namespace(managed("other"));
    bed.until("all synced", |views| {
        ["sbx", "other"]
            .iter()
            .all(|ns| views.pods(ns).observed().is_some() && views.pools(ns).observed().is_some())
    })
    .await;
    bed.kube
        .break_watch("pools", "sbx", "the pool watch broke: down");
    bed.until(
        "pools of sbx not synced",
        |views| matches!(views.pools("sbx"), Read::NotObserved(reason) if reason.contains("down")),
    )
    .await;
    assert!(bed.views.pools("other").observed().is_some());
    assert!(bed.views.pods("sbx").observed().is_some());
    bed.kube.resync_watch("pools", "sbx");
    bed.until("pools of sbx synced again", |views| {
        views.pools("sbx").observed().is_some()
    })
    .await;
    bed.kube.break_watch("pods", "sbx", "the pod watch broke");
    bed.until("pods of sbx not synced", |views| {
        views.pods("sbx").observed().is_none()
    })
    .await;
    bed.kube
        .break_watch("namespaces", "", "the namespace list failed");
    bed.until("namespaces not synced", |views| {
        views.managed_namespaces().observed().is_none()
    })
    .await;
    assert!(bed.views.pods("other").observed().is_some());
    bed.kube
        .break_watch("bindings", "", "the binding watch broke");
    bed.until("binding not synced", |views| {
        views.binding().observed().is_none()
    })
    .await;
    bed.kube.resync_watch("namespaces", "");
    bed.kube.resync_watch("bindings", "");
    bed.kube.resync_watch("pods", "sbx");
    bed.until("everything synced again", |views| {
        views.managed_namespaces().observed().is_some()
            && views.binding().observed().is_some()
            && views.pods("sbx").observed().is_some()
    })
    .await;
}

#[tokio::test]
async fn a_namespace_that_leaves_the_selector_loses_its_views_and_its_watches_end() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    bed.until("sbx synced", |views| views.pods("sbx").observed().is_some())
        .await;
    bed.kube.put_namespace(namespace("sbx", &[("team", "x")]));
    bed.until("sbx dropped", |views| {
        matches!(views.pods("sbx"), Read::NotObserved(reason) if reason.contains("not watched"))
            && matches!(views.namespace("sbx"), Read::Observed(None))
    })
    .await;
    bed.kube.put_namespace(managed("sbx"));
    bed.until("sbx watched again", |views| {
        views.pods("sbx").observed().is_some()
    })
    .await;
    assert_eq!(bed.kube.watch_opens("pools", "sbx"), 2);
}

#[tokio::test]
async fn the_binding_is_read_by_its_name_and_goes_when_it_is_removed() {
    let bed = Bed::start();
    bed.kube.put_binding(binding());
    bed.until("binding seen", |views| {
        matches!(views.binding(), Read::Observed(Some(_)))
    })
    .await;
    bed.kube.remove_binding("br-sandbox-pod");
    bed.until("binding gone", |views| {
        matches!(views.binding(), Read::Observed(None))
    })
    .await;
}

#[tokio::test]
async fn the_driver_ends_when_it_is_told_to_stop() {
    let kube = Arc::new(crate::fakes::FakeKube::default());
    let api: Arc<dyn PoolApi> = kube;
    let stop = CancellationToken::new();
    let task =
        tokio::spawn(Views::new().run(api, "br.sandbox/managed=true".to_owned(), stop.clone()));
    stop.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("the driver ends")
        .unwrap();
}

#[tokio::test]
async fn a_namespace_that_is_deleted_loses_its_views() {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    bed.until("sbx synced", |views| {
        views.pools("sbx").observed().is_some()
    })
    .await;
    bed.kube.remove_namespace("sbx");
    bed.until("sbx dropped", |views| {
        views.pools("sbx").observed().is_none()
    })
    .await;
    assert!(matches!(bed.views.namespace("sbx"), Read::Observed(None)));
}
