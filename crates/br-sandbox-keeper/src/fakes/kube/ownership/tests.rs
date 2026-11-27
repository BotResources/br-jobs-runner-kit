use super::*;

const ONE: &str = "manager-one";
const TWO: &str = "manager-two";

fn created(image: &str) -> (ReplicaSet, Value) {
    let body = json!({
        "apiVersion": "apps/v1",
        "kind": "ReplicaSet",
        "metadata": {"name": "warm"},
        "spec": {
            "replicas": 1,
            "selector": {"matchLabels": {"pool": "warm"}},
            "template": {
                "metadata": {"labels": {"pool": "warm"}},
                "spec": {"containers": [{"name": "agent", "image": image}]}
            }
        }
    });
    let mut set: ReplicaSet = serde_json::from_value(body.clone()).unwrap();
    record_create(&mut set, ONE, &body);
    (set, body)
}

fn template_of(image: &str) -> Value {
    json!({"spec": {"template": {
        "metadata": {"labels": {"pool": "warm"}},
        "spec": {"containers": [{"name": "agent", "image": image}]}
    }}})
}

fn replicas(count: i32) -> Value {
    json!({"spec": {"replicas": count}})
}

fn image_of(set: &ReplicaSet) -> Option<String> {
    set.spec
        .as_ref()?
        .template
        .as_ref()?
        .spec
        .as_ref()?
        .containers
        .first()?
        .image
        .clone()
}

fn owners_of(set: &ReplicaSet, path: &[&str]) -> Vec<(String, String)> {
    let wanted: Path = path.iter().map(|segment| (*segment).to_owned()).collect();
    entries_of(set)
        .into_iter()
        .filter(|entry| entry.paths.contains(&wanted))
        .map(|entry| (entry.manager, entry.operation))
        .collect()
}

fn image_path() -> Vec<&'static str> {
    vec![
        "f:spec",
        "f:template",
        "f:spec",
        "f:containers",
        "k:{\"name\":\"agent\"}",
        "f:image",
    ]
}

#[test]
fn the_create_is_an_update_entry_that_owns_every_field_of_the_body() {
    let (set, _) = created("img:1");
    assert_eq!(
        owners_of(&set, &image_path()),
        [(ONE.to_owned(), UPDATE.to_owned())]
    );
    assert_eq!(
        owners_of(&set, &["f:spec", "f:replicas"]),
        [(ONE.to_owned(), UPDATE.to_owned())]
    );
}

#[test]
fn the_entries_survive_a_trip_through_the_managed_fields_of_the_object() {
    let (set, _) = created("img:1");
    let before = entries_of(&set);
    let mut copy = set.clone();
    store(&mut copy, entries_of(&set));
    let after = entries_of(&copy);
    assert_eq!(before.len(), after.len());
    assert_eq!(before[0].paths, after[0].paths);
}

#[test]
fn a_later_apply_removes_the_fields_that_the_same_manager_owned_alone_and_no_longer_holds() {
    let (mut set, _) = created("img:1");
    apply(&mut set, TWO, &template_of("img:2")).unwrap();
    assert_eq!(
        owners_of(&set, &image_path()),
        [(TWO.to_owned(), APPLY.to_owned())]
    );
    let refused = apply(&mut set, TWO, &replicas(5));
    let Err(ApiError::Call(message)) = refused else {
        panic!("the apply that drops the image must be refused: {refused:?}");
    };
    assert!(message.contains("422"));
    assert!(message.contains("spec.template.spec.containers[0].image: Required value"));
    assert_eq!(image_of(&set).as_deref(), Some("img:2"));
}

#[test]
fn a_refused_apply_changes_nothing_of_the_object_or_of_its_owners() {
    let (mut set, _) = created("img:1");
    apply(&mut set, TWO, &template_of("img:2")).unwrap();
    let before = serde_json::to_value(&set).unwrap();
    assert!(apply(&mut set, TWO, &replicas(5)).is_err());
    assert_eq!(serde_json::to_value(&set).unwrap(), before);
}

#[test]
fn a_manager_that_applies_other_fields_never_removes_the_fields_of_another_manager() {
    let (mut set, _) = created("img:1");
    apply(&mut set, TWO, &template_of("img:2")).unwrap();
    apply(&mut set, ONE, &replicas(5)).unwrap();
    apply(&mut set, ONE, &replicas(7)).unwrap();
    assert_eq!(image_of(&set).as_deref(), Some("img:2"));
    assert_eq!(set.spec.as_ref().unwrap().replicas, Some(7));
    apply(&mut set, TWO, &template_of("img:3")).unwrap();
    assert_eq!(set.spec.as_ref().unwrap().replicas, Some(7));
    assert_eq!(image_of(&set).as_deref(), Some("img:3"));
}

#[test]
fn a_field_that_another_entry_owns_too_stays_when_one_manager_stops_applying_it() {
    let (mut set, _) = created("img:1");
    apply(&mut set, TWO, &template_of("img:1")).unwrap();
    assert_eq!(owners_of(&set, &image_path()).len(), 2);
    apply(&mut set, TWO, &replicas(5)).unwrap();
    assert_eq!(image_of(&set).as_deref(), Some("img:1"));
    assert_eq!(
        owners_of(&set, &image_path()),
        [(ONE.to_owned(), UPDATE.to_owned())]
    );
}

#[test]
fn a_force_apply_of_a_different_value_takes_the_field_from_the_other_entries() {
    let (mut set, _) = created("img:1");
    apply(&mut set, TWO, &template_of("img:2")).unwrap();
    assert_eq!(
        owners_of(&set, &image_path()),
        [(TWO.to_owned(), APPLY.to_owned())]
    );
    assert_eq!(
        owners_of(&set, &["f:spec", "f:replicas"]),
        [(ONE.to_owned(), UPDATE.to_owned())]
    );
}

#[test]
fn an_object_without_a_template_is_not_judged_on_its_containers() {
    let mut set: ReplicaSet = serde_json::from_value(json!({
        "metadata": {"name": "warm"},
        "spec": {"replicas": 1, "selector": {}}
    }))
    .unwrap();
    apply(&mut set, ONE, &replicas(3)).unwrap();
    assert_eq!(set.spec.as_ref().unwrap().replicas, Some(3));
}
