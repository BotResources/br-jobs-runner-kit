use std::time::Duration;

use br_runner_sandbox::Timestamp;

use crate::time::elapsed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScaleDownRules {
    pub cooldown: Duration,
    pub step: u32,
    pub interval: Duration,
}

impl ScaleDownRules {
    pub fn new(cooldown: Duration, step: u32, interval: Duration) -> Self {
        Self {
            cooldown,
            step,
            interval,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScaleState {
    below_since: Option<Timestamp>,
    last_step: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Damped {
    pub replicas: u32,
    pub state: ScaleState,
    pub wake_at: Option<Timestamp>,
}

pub fn damp(
    state: ScaleState,
    current: u32,
    desired: u32,
    now: Timestamp,
    rules: &ScaleDownRules,
) -> Damped {
    if desired >= current {
        return Damped {
            replicas: desired,
            state: ScaleState::default(),
            wake_at: None,
        };
    }
    let below_since = state.below_since.unwrap_or(now);
    let cooldown_ends = below_since.plus(rules.cooldown);
    let waiting = |state: ScaleState, wake_at| Damped {
        replicas: current,
        state,
        wake_at: Some(wake_at),
    };
    let state = ScaleState {
        below_since: Some(below_since),
        ..state
    };
    if elapsed(now, below_since) < rules.cooldown {
        return waiting(state, cooldown_ends);
    }
    if let Some(last) = state.last_step {
        let next = last.plus(rules.interval);
        if elapsed(now, last) < rules.interval {
            return waiting(state, next);
        }
    }
    let replicas = desired.max(current.saturating_sub(rules.step));
    Damped {
        replicas,
        state: ScaleState {
            last_step: Some(now),
            ..state
        },
        wake_at: (replicas > desired).then(|| now.plus(rules.interval)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::at;

    fn rules() -> ScaleDownRules {
        ScaleDownRules {
            cooldown: Duration::from_secs(120),
            step: 5,
            interval: Duration::from_secs(10),
        }
    }

    fn run(current: u32, desired: u32, until_second: u64) -> (u32, Option<u64>) {
        run_with(&rules(), current, desired, until_second)
    }

    fn run_with(
        rules: &ScaleDownRules,
        mut current: u32,
        desired: u32,
        until_second: u64,
    ) -> (u32, Option<u64>) {
        let mut state = ScaleState::default();
        let mut reached = None;
        for second in 0..=until_second {
            let damped = damp(state, current, desired, at(1000 + second), rules);
            state = damped.state;
            current = damped.replicas;
            if current == desired && reached.is_none() {
                reached = Some(second);
            }
        }
        (current, reached)
    }

    #[test]
    fn a_scale_up_is_applied_at_once() {
        let damped = damp(ScaleState::default(), 3, 10, at(1000), &rules());
        assert_eq!(damped.replicas, 10);
        assert_eq!(damped.wake_at, None);
        assert_eq!(damped.state, ScaleState::default());
    }

    #[test]
    fn an_equal_size_changes_nothing() {
        assert_eq!(
            damp(ScaleState::default(), 5, 5, at(1000), &rules()).replicas,
            5
        );
    }

    #[test]
    fn a_scale_down_waits_for_the_cooldown() {
        let first = damp(ScaleState::default(), 20, 2, at(1000), &rules());
        assert_eq!(first.replicas, 20);
        assert_eq!(first.wake_at, Some(at(1120)));
        let later = damp(first.state, 20, 2, at(1119), &rules());
        assert_eq!(later.replicas, 20);
        assert_eq!(later.wake_at, Some(at(1120)));
    }

    #[test]
    fn after_the_cooldown_the_replicas_fall_by_at_most_one_step() {
        let first = damp(ScaleState::default(), 20, 2, at(1000), &rules());
        let step = damp(first.state, 20, 2, at(1120), &rules());
        assert_eq!(step.replicas, 15);
        assert_eq!(step.wake_at, Some(at(1130)));
        let early = damp(step.state, 15, 2, at(1125), &rules());
        assert_eq!(early.replicas, 15);
        assert_eq!(early.wake_at, Some(at(1130)));
        let second = damp(early.state, 15, 2, at(1130), &rules());
        assert_eq!(second.replicas, 10);
    }

    #[test]
    fn the_last_step_never_goes_below_the_desired_size() {
        let first = damp(ScaleState::default(), 7, 5, at(1000), &rules());
        let step = damp(first.state, 7, 5, at(1120), &rules());
        assert_eq!(step.replicas, 5);
        assert_eq!(step.wake_at, None);
    }

    #[test]
    fn a_floor_of_95_reaches_0_within_300_seconds_of_the_fall() {
        let (current, reached) = run(95, 0, 300);
        assert_eq!(current, 0);
        assert_eq!(reached, Some(300));
        let (current, _) = run(95, 0, 299);
        assert_eq!(current, 5);
    }

    #[test]
    fn the_default_configuration_takes_a_floor_of_95_to_0_within_300_seconds() {
        let config = crate::testing::config();
        let defaults = ScaleDownRules::new(
            config.scale_down_cooldown,
            config.scale_down_step,
            config.scale_down_step_interval,
        );
        assert_eq!(
            (defaults.cooldown, defaults.step, defaults.interval),
            (Duration::from_secs(120), 5, Duration::from_secs(10))
        );
        let (current, reached) = run_with(&defaults, 95, 0, 300);
        assert_eq!(current, 0);
        assert!(reached.is_some_and(|second| second <= 300));
        let (current, _) = run_with(&defaults, 95, 0, 299);
        assert_ne!(current, 0);
    }

    #[test]
    fn a_desired_size_that_is_not_below_the_replicas_ends_the_cooldown() {
        let first = damp(ScaleState::default(), 20, 2, at(1000), &rules());
        let back = damp(first.state, 20, 20, at(1100), &rules());
        assert_eq!(back.state, ScaleState::default());
        let again = damp(back.state, 20, 2, at(1110), &rules());
        assert_eq!(again.replicas, 20);
        assert_eq!(again.wake_at, Some(at(1230)));
        let after_old_cooldown = damp(again.state, 20, 2, at(1125), &rules());
        assert_eq!(after_old_cooldown.replicas, 20);
    }

    #[test]
    fn a_new_claim_that_keeps_the_desired_size_below_the_replicas_gives_no_new_cooldown() {
        let first = damp(ScaleState::default(), 30, 2, at(1000), &rules());
        let step = damp(first.state, 30, 2, at(1120), &rules());
        assert_eq!(step.replicas, 25);
        let busier = damp(step.state, 25, 12, at(1130), &rules());
        assert_eq!(busier.replicas, 20);
    }

    #[test]
    fn a_scale_down_of_one_replica_goes_by_the_same_cooldown() {
        let (current, reached) = run(3, 2, 200);
        assert_eq!(current, 2);
        assert_eq!(reached, Some(120));
    }
}
