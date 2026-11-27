use super::*;

fn view() -> View<String> {
    View::new(|item: &String| item.clone())
}

fn names(read: Read<Vec<String>>) -> Vec<String> {
    read.observed().expect("a synced view")
}

#[test]
fn a_view_is_not_synced_before_its_first_list_and_reads_nothing() {
    let mut view = view();
    assert!(!view.is_synced());
    assert!(matches!(view.read("pods of sbx"), Read::NotObserved(reason)
        if reason.contains("pods of sbx") && reason.contains("first list")));
    view.apply(ViewEvent::Applied("w0".to_owned()));
    assert!(!view.is_synced());
    assert!(view.read("pods").observed().is_none());
}

#[test]
fn the_snapshot_syncs_the_view_and_replaces_every_item() {
    let mut view = view();
    view.apply(ViewEvent::Snapshot(vec!["a".to_owned(), "b".to_owned()]));
    assert!(view.is_synced());
    assert_eq!(names(view.read("x")), ["a", "b"]);
    view.apply(ViewEvent::Snapshot(vec!["c".to_owned()]));
    assert_eq!(names(view.read("x")), ["c"]);
}

#[test]
fn a_change_and_a_delete_after_the_snapshot_are_folded() {
    let mut view = view();
    view.apply(ViewEvent::Snapshot(vec!["a".to_owned()]));
    view.apply(ViewEvent::Applied("b".to_owned()));
    view.apply(ViewEvent::Deleted {
        key: "a".to_owned(),
    });
    assert_eq!(names(view.read("x")), ["b"]);
    assert_eq!(
        view.read_one("x", "b"),
        Read::Observed(Some("b".to_owned()))
    );
    assert_eq!(view.read_one("x", "a"), Read::Observed(None));
}

#[test]
fn a_failure_unsyncs_the_view_and_names_the_reason_until_the_next_snapshot() {
    let mut view = view();
    view.apply(ViewEvent::Snapshot(vec!["a".to_owned()]));
    view.apply(ViewEvent::Failed {
        reason: "the pod list failed: down".to_owned(),
    });
    assert!(!view.is_synced());
    assert!(matches!(view.read("pods"), Read::NotObserved(reason) if reason.contains("down")));
    assert!(matches!(view.read_one("pods", "a"), Read::NotObserved(_)));
    view.apply(ViewEvent::Applied("b".to_owned()));
    assert!(!view.is_synced());
    view.apply(ViewEvent::Snapshot(vec!["b".to_owned()]));
    assert!(view.is_synced());
    assert_eq!(names(view.read("pods")), ["b"]);
}

#[test]
fn a_missing_kind_unsyncs_the_view_and_marks_it_absent_until_the_next_snapshot() {
    let mut view = view();
    view.apply(ViewEvent::Snapshot(vec!["a".to_owned()]));
    assert!(!view.is_absent());
    view.apply(ViewEvent::Missing {
        reason: "the pool resource list failed".to_owned(),
    });
    assert!(view.is_absent());
    assert!(!view.is_synced());
    assert!(
        matches!(view.unsynced_reason("pools"), Some(reason) if reason.contains("pool resource list"))
    );
    view.apply(ViewEvent::Failed {
        reason: "down".to_owned(),
    });
    assert!(!view.is_absent());
    view.apply(ViewEvent::Missing {
        reason: "gone".to_owned(),
    });
    view.apply(ViewEvent::Snapshot(Vec::new()));
    assert!(!view.is_absent());
    assert!(view.is_synced());
    assert_eq!(view.unsynced_reason("pools"), None);
}
