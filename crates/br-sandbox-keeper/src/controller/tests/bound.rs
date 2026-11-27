use std::time::Duration;

use super::rollout::{current, stale_bed};
use super::*;

const T: u64 = 30;
const LIMIT: u64 = 400;

fn note_deletes(bed: &Bed, seen: &mut usize, due: &mut Vec<(u64, String)>, second: u64) {
    let deletes = bed.deletes();
    for name in &deletes[*seen..] {
        due.push((second + T, format!("r-{name}")));
    }
    *seen = deletes.len();
}

async fn rollout_end(gap: Option<(u64, u64)>) -> u64 {
    let mut bed = stale_bed(2).await;
    let mut seen = 0;
    let mut due: Vec<(u64, String)> = Vec::new();
    for second in 0..=LIMIT {
        if gap.is_some_and(|(start, _)| start == second) {
            bed.leadership.lost();
        }
        if gap.is_some_and(|(_, end)| end == second) {
            bed.leadership.renewed(at(START + 100_000_000));
            bed.engine.gained_leadership().await;
        }
        if second > 0 {
            bed.pass(1).await;
        }
        note_deletes(&bed, &mut seen, &mut due, second);
        let (ready, waiting): (Vec<_>, Vec<_>) = due
            .drain(..)
            .partition(|(at_second, _)| *at_second <= second);
        due = waiting;
        for (_, name) in ready {
            let pod = current(&bed, &name, true);
            bed.kube.put_pod(pod);
            bed.settle().await;
            note_deletes(&bed, &mut seen, &mut due, second);
        }
        let replaced = (0..4)
            .filter(|index| bed.kube.pod(NS, &format!("r-w{index}")).is_some())
            .count();
        if replaced == 4 {
            return second;
        }
    }
    panic!("the rollout did not end");
}

#[tokio::test(start_paused = true)]
async fn a_rollout_that_goes_well_ends_within_the_bound_of_the_steps() {
    let ended = rollout_end(None).await;
    let bucket = 10;
    let steps = 2;
    assert!(ended <= steps * (T + bucket), "{ended}");
}

#[tokio::test(start_paused = true)]
async fn a_change_of_leader_adds_at_most_the_lease_duration_to_the_rollout() {
    let lease = bed(true).engine.config.lease.duration;
    assert_eq!(lease, Duration::from_secs(15));
    let baseline = rollout_end(None).await;
    for start in [5, 20, 29, 31, 45] {
        let ended = rollout_end(Some((start, start + lease.as_secs()))).await;
        assert!(
            ended <= baseline + lease.as_secs(),
            "gap from {start}: {ended} against {baseline}"
        );
    }
    let delayed = rollout_end(Some((29, 29 + lease.as_secs()))).await;
    assert!(delayed > baseline);
}
