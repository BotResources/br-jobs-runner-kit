use serde_json::{Value, json};

use super::*;
use crate::cluster::FIELD_MANAGER;

async fn created(rig: &Rig, replicas: u32, image: &str) -> RenderedTemplate {
    let first = rendered(image);
    assert_eq!(
        rig.writer
            .create_replica_set("sbx", &owner(), replicas, &first)
            .await,
        Ok(CreateOutcome::Created)
    );
    rig.kube.clear_calls();
    first
}

fn held(rig: &Rig) -> (Option<i32>, Option<String>) {
    let set = rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap();
    let spec = set.spec.unwrap();
    let image = spec
        .template
        .and_then(|template| template.spec)
        .and_then(|pod| pod.containers.into_iter().next())
        .and_then(|container| container.image);
    (spec.replicas, image)
}

fn template_body(rendered: &RenderedTemplate) -> Value {
    json!({"spec": {"template": rendered.template}})
}

fn replicas_body(replicas: i32) -> Value {
    json!({"spec": {"replicas": replicas}})
}

#[tokio::test]
async fn a_pool_whose_template_changed_can_still_change_its_replicas_and_keeps_its_image() {
    let rig = rig();
    created(&rig, 2, "img:1").await;
    assert_eq!(
        rig.writer
            .set_template("sbx", &owner(), &rendered("img:2"))
            .await,
        Ok(Change::Written)
    );
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 5).await,
        Ok(Change::Written)
    );
    assert_eq!(held(&rig), (Some(5), Some("img:2".to_owned())));
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 6).await,
        Ok(Change::Written)
    );
    assert_eq!(held(&rig), (Some(6), Some("img:2".to_owned())));
}

#[tokio::test]
async fn a_pool_whose_replicas_changed_keeps_them_through_every_template_change() {
    let rig = rig();
    created(&rig, 2, "img:1").await;
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 5).await,
        Ok(Change::Written)
    );
    for image in ["img:2", "img:3", "img:1"] {
        assert_eq!(
            rig.writer
                .set_template("sbx", &owner(), &rendered(image))
                .await,
            Ok(Change::Written)
        );
        assert_eq!(held(&rig), (Some(5), Some(image.to_owned())));
    }
}

#[tokio::test]
async fn a_pool_that_never_had_a_template_write_scales_and_keeps_scaling_after_one() {
    let rig = rig();
    created(&rig, 2, "img:1").await;
    for replicas in [4, 6, 3] {
        assert_eq!(
            rig.writer.set_replicas("sbx", &owner(), replicas).await,
            Ok(Change::Written)
        );
    }
    assert_eq!(held(&rig), (Some(3), Some("img:1".to_owned())));
    assert_eq!(
        rig.writer
            .set_template("sbx", &owner(), &rendered("img:2"))
            .await,
        Ok(Change::Written)
    );
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 8).await,
        Ok(Change::Written)
    );
    assert_eq!(held(&rig), (Some(8), Some("img:2".to_owned())));
}

#[tokio::test]
async fn an_unchanged_template_on_a_replica_set_that_the_keeper_made_is_written_once_and_then_not_again()
 {
    let rig = rig();
    let first = created(&rig, 2, "img:1").await;
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), &first).await,
        Ok(Change::Written)
    );
    assert_eq!(rig.kube.calls().len(), 1);
    for _ in 0..3 {
        assert_eq!(
            rig.writer.set_template("sbx", &owner(), &first).await,
            Ok(Change::Unchanged)
        );
    }
    assert_eq!(rig.kube.calls().len(), 1);
}

#[tokio::test]
async fn a_replica_set_whose_template_the_previous_version_applied_under_the_replicas_manager_is_moved_by_one_template_write()
 {
    let rig = rig();
    let first = created(&rig, 2, "img:1").await;
    for step in [rendered("img:2"), first.clone()] {
        rig.kube
            .apply_as(FIELD_MANAGER, "sbx", "warm-sbx", &template_body(&step))
            .unwrap();
    }
    assert!(
        rig.kube
            .apply_as(FIELD_MANAGER, "sbx", "warm-sbx", &replicas_body(5))
            .is_err(),
        "the state of the previous version cannot take a replicas apply"
    );
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), &first).await,
        Ok(Change::Written)
    );
    assert_eq!(rig.kube.calls().len(), 1);
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 5).await,
        Ok(Change::Written)
    );
    assert_eq!(held(&rig), (Some(5), Some("img:1".to_owned())));
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), &first).await,
        Ok(Change::Unchanged)
    );
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 7).await,
        Ok(Change::Written)
    );
    assert_eq!(held(&rig), (Some(7), Some("img:1".to_owned())));
    let writes: Vec<_> = rig
        .kube
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::ApplyTemplate { .. }))
        .collect();
    assert_eq!(writes.len(), 1);
}
