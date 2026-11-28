use crate::cluster::{FIELD_MANAGER, TEMPLATE_FIELD_MANAGER};

use super::*;

fn image_of(bed: &Bed) -> Option<String> {
    bed.kube
        .raw_replica_set(NS, RS)?
        .spec?
        .template?
        .spec?
        .containers
        .first()?
        .image
        .clone()
}

async fn made_by_the_keeper() -> Bed {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
    bed
}

fn with_image(image: &str, generation: i64) -> SandboxPool {
    let mut changed = spec();
    changed.image = image.to_owned();
    edited(&changed, generation)
}

fn with_minimum(minimum: u32, image: &str, generation: i64) -> SandboxPool {
    let mut changed = spec();
    changed.image = image.to_owned();
    changed.min_replicas = minimum;
    edited(&changed, generation)
}

#[tokio::test(start_paused = true)]
async fn a_pool_whose_image_was_edited_twice_can_still_be_resized() {
    let mut bed = made_by_the_keeper().await;
    let original = image_of(&bed).unwrap();
    bed.kube
        .put_pool(NS, with_image("registry.example/agent:2", 2));
    bed.settle().await;
    bed.kube.put_pool(NS, with_image(&original, 3));
    bed.settle().await;
    assert_eq!(image_of(&bed).as_deref(), Some(original.as_str()));
    bed.kube.clear_calls();
    bed.kube.put_pool(NS, with_minimum(5, &original, 4));
    bed.settle().await;
    assert_eq!(bed.replica_writes(), vec![5]);
    assert_eq!(bed.replicas(), Some(5));
    assert_eq!(image_of(&bed).as_deref(), Some(original.as_str()));
}

#[tokio::test(start_paused = true)]
async fn a_pool_that_was_resized_keeps_its_replicas_when_its_image_changes() {
    let mut bed = made_by_the_keeper().await;
    let original = image_of(&bed).unwrap();
    bed.kube.put_pool(NS, with_minimum(5, &original, 2));
    bed.settle().await;
    assert_eq!(bed.replicas(), Some(5));
    bed.kube
        .put_pool(NS, with_minimum(5, "registry.example/agent:2", 3));
    bed.settle().await;
    assert_eq!(bed.replicas(), Some(5));
    assert_eq!(image_of(&bed).as_deref(), Some("registry.example/agent:2"));
}

#[tokio::test(start_paused = true)]
async fn a_pool_whose_template_the_previous_version_wrote_is_moved_once_and_then_resized() {
    let mut bed = made_by_the_keeper().await;
    let original = image_of(&bed).unwrap();
    bed.kube
        .put_pool(NS, with_image("registry.example/agent:2", 2));
    bed.settle().await;
    bed.kube.put_pool(NS, with_image(&original, 3));
    bed.settle().await;
    bed.kube
        .rename_field_manager(NS, RS, TEMPLATE_FIELD_MANAGER, FIELD_MANAGER);
    bed.kube.clear_calls();
    bed.kube.put_pool(NS, with_minimum(5, &original, 4));
    bed.settle().await;
    assert_eq!(bed.template_writes(), 1);
    assert_eq!(bed.replica_writes(), vec![5]);
    assert_eq!(bed.replicas(), Some(5));
    assert_eq!(image_of(&bed).as_deref(), Some(original.as_str()));
    bed.kube.clear_calls();
    bed.pass(30).await;
    assert_eq!(bed.template_writes(), 0);
    bed.kube.put_pool(NS, with_minimum(7, &original, 5));
    bed.settle().await;
    assert_eq!(bed.replicas(), Some(7));
    assert_eq!(bed.template_writes(), 0);
}
