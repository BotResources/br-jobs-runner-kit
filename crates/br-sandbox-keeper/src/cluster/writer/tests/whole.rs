use serde_json::{Value, json};

use super::*;
use crate::pool::ResourceBounds;

type Edit = fn(&mut PoolSettings);

const SIZE_EDITS: [(&str, Edit); 6] = [
    ("cpu request", |wanted| {
        wanted.size.cpu = ResourceBounds::new("250m", wanted.size.cpu.limit.0.clone());
    }),
    ("cpu limit", |wanted| {
        wanted.size.cpu = ResourceBounds::new(wanted.size.cpu.request.0.clone(), "3");
    }),
    ("memory request", |wanted| {
        wanted.size.memory = ResourceBounds::new("2Gi", wanted.size.memory.limit.0.clone());
    }),
    ("memory limit", |wanted| {
        wanted.size.memory = ResourceBounds::new(wanted.size.memory.request.0.clone(), "6Gi");
    }),
    ("disk request", |wanted| {
        wanted.size.disk = ResourceBounds::new("2Gi", wanted.size.disk.limit.0.clone());
    }),
    ("disk limit", |wanted| {
        wanted.size.disk = ResourceBounds::new(wanted.size.disk.request.0.clone(), "10Gi");
    }),
];

fn document(mode: &str, grace: Option<u32>) -> Value {
    let mut document = template_json("sbx");
    document["spec"]["containers"][0]["env"] = json!([{"name": "MODE", "value": mode}]);
    if let Some(seconds) = grace {
        document["spec"]["terminationGracePeriodSeconds"] = json!(seconds);
    }
    document
}

fn with_edit(edit: Edit) -> PoolSettings {
    let mut wanted = settings();
    edit(&mut wanted);
    wanted
}

pub(super) fn all_size_edits() -> PoolSettings {
    let mut wanted = settings();
    for (_, edit) in SIZE_EDITS {
        edit(&mut wanted);
    }
    wanted
}

pub(super) fn template_of(rig: &Rig) -> Value {
    let set = rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap();
    serde_json::to_value(set.spec.unwrap().template.unwrap()).unwrap()
}

fn replicas_of(rig: &Rig) -> Option<i32> {
    rig.kube
        .raw_replica_set("sbx", "warm-sbx")
        .and_then(|set| set.spec)
        .and_then(|spec| spec.replicas)
}

pub(super) async fn made(rig: &Rig, first: &RenderedTemplate) {
    assert_eq!(
        rig.writer
            .create_replica_set("sbx", &owner(), 2, first)
            .await,
        Ok(CreateOutcome::Created)
    );
    rig.kube.clear_calls();
}

async fn change_template_then_replicas(rig: &Rig, next: &RenderedTemplate, replicas: u32) {
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), next).await,
        Ok(Change::Written)
    );
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), replicas).await,
        Ok(Change::Written)
    );
    assert_eq!(template_of(rig), next.template);
    assert_eq!(replicas_of(rig), i32::try_from(replicas).ok());
}

async fn assert_every_template_change_keeps_the_template_whole(
    first: RenderedTemplate,
    changes: Vec<RenderedTemplate>,
) {
    let rig = rig();
    made(&rig, &first).await;
    for (replicas, next) in (3..).zip(changes.iter().chain([&first])) {
        change_template_then_replicas(&rig, next, replicas).await;
    }
}

#[tokio::test]
async fn an_edit_of_each_size_field_followed_by_a_change_of_the_replicas_keeps_the_rendered_template()
 {
    for (field, edit) in SIZE_EDITS {
        let base = rendered_with(&settings(), &template_json("sbx"));
        let edited = rendered_with(&with_edit(edit), &template_json("sbx"));
        assert_ne!(base.template, edited.template, "{field}");
        assert_every_template_change_keeps_the_template_whole(base, vec![edited]).await;
    }
}

#[tokio::test]
async fn an_edit_of_the_whole_size_followed_by_a_change_of_the_replicas_keeps_the_rendered_template()
 {
    let base = rendered_with(&settings(), &template_json("sbx"));
    let edited = rendered_with(&all_size_edits(), &template_json("sbx"));
    assert_every_template_change_keeps_the_template_whole(base, vec![edited]).await;
}

#[tokio::test]
async fn an_edit_of_the_image_followed_by_a_change_of_the_replicas_keeps_the_rendered_template() {
    assert_every_template_change_keeps_the_template_whole(
        rendered("img:1"),
        vec![rendered("img:2"), rendered("img:3")],
    )
    .await;
}

#[tokio::test]
async fn a_change_of_the_template_configmap_followed_by_a_change_of_the_replicas_keeps_the_rendered_template()
 {
    let base = rendered_with(&settings(), &document("a", None));
    let changes = vec![
        rendered_with(&settings(), &document("b", None)),
        rendered_with(&settings(), &document("b", Some(45))),
        rendered_with(&all_size_edits(), &document("c", Some(60))),
    ];
    assert_every_template_change_keeps_the_template_whole(base, changes).await;
}
