use super::*;
use crate::convert::time_of;

fn base() -> SandboxPoolStatus {
    status_of(&Acceptance::Valid, Some(&observed(2, 100)), None, at(1000))
}

#[test]
fn the_first_status_is_written() {
    let next = base();
    assert_eq!(
        decide_write(next.clone(), None, None, at(1000)),
        StatusWrite::Write(next)
    );
}

#[test]
fn nothing_is_written_when_no_value_changed_and_a_newer_observed_at_alone_is_no_change() {
    let current = base();
    let mut later = status_of(&Acceptance::Valid, Some(&observed(2, 160)), Some(&current), at(1060));
    assert_ne!(later.observed_at, current.observed_at);
    assert_eq!(decide_write(later.clone(), Some(&current), None, at(1060)), StatusWrite::Skip);
    later.observed_at = Some(time_of(at(9999)));
    assert_eq!(decide_write(later, Some(&current), None, at(1060)), StatusWrite::Skip);
}

#[test]
fn a_changed_count_a_condition_or_a_generation_is_a_change() {
    let current = base();
    let more = status_of(&Acceptance::Valid, Some(&observed(3, 160)), Some(&current), at(1060));
    assert!(matches!(
        decide_write(more, Some(&current), None, at(1060)),
        StatusWrite::Write(_)
    ));
    let refused = status_of(
        &Acceptance::DuplicatePool,
        Some(&observed(2, 160)),
        Some(&current),
        at(1060),
    );
    assert!(matches!(
        decide_write(refused, Some(&current), None, at(1060)),
        StatusWrite::Write(_)
    ));
    let mut generation = current.clone();
    generation.observed_generation = Some(8);
    assert!(matches!(
        decide_write(generation, Some(&current), None, at(1060)),
        StatusWrite::Write(_)
    ));
}

#[test]
fn a_change_within_five_seconds_of_the_last_write_waits_for_the_fifth_second() {
    let current = base();
    let changed = status_of(&Acceptance::Valid, Some(&observed(3, 160)), Some(&current), at(1060));
    for now in [1000, 1001, 1004] {
        assert_eq!(
            decide_write(changed.clone(), Some(&current), Some(at(1000)), at(now)),
            StatusWrite::Wait { until: at(1005) },
            "{now}"
        );
    }
}

#[test]
fn a_change_five_seconds_after_the_last_write_is_written() {
    let current = base();
    let changed = status_of(&Acceptance::Valid, Some(&observed(3, 160)), Some(&current), at(1060));
    for now in [1005, 1006, 5000] {
        assert_eq!(
            decide_write(changed.clone(), Some(&current), Some(at(1000)), at(now)),
            StatusWrite::Write(changed.clone()),
            "{now}"
        );
    }
}

#[test]
fn a_restart_that_lost_the_last_write_instant_may_write_at_once() {
    let current = base();
    let changed = status_of(&Acceptance::Valid, Some(&observed(3, 160)), Some(&current), at(1060));
    assert!(matches!(
        decide_write(changed, Some(&current), None, at(1001)),
        StatusWrite::Write(_)
    ));
}

#[test]
fn at_most_one_write_in_five_seconds_for_a_run_of_changes() {
    let mut current = base();
    let mut last = None;
    let mut writes = 0;
    for second in 0..30u64 {
        let now = at(2000 + second);
        let next = status_of(
            &Acceptance::Valid,
            Some(&observed(u32::try_from(second % 4).unwrap() + 10, 2000 + second)),
            Some(&current),
            now,
        );
        if let StatusWrite::Write(written) = decide_write(next, Some(&current), last, now) {
            writes += 1;
            last = Some(now);
            current = written;
        }
    }
    assert!(writes <= 6, "{writes}");
    assert!(writes >= 5, "{writes}");
}
