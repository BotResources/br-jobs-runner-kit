use serde_json::{Value, json};

use crate::chart_line::ChartLine;
use crate::env::Env;
use crate::error::Res;
use crate::guards::{PoolGuard, ValueGuard};
use crate::pool;

const WARM_WAIT_SECONDS: u64 = 180;
const DEFAULT_FLOOR: u64 = 1;

pub struct FloorGuard {
    _values: Option<ValueGuard>,
    _pool: Option<PoolGuard>,
}

pub fn floor_key(env: &Env) -> String {
    format!("clients.{}.modes.{}.pool.floor", env.client, env.mode)
}

pub fn floor_sets(spec: &Value, want: u64) -> Vec<(String, Value)> {
    let min = spec["minReplicas"].as_u64().unwrap_or(0);
    let mut sets = Vec::new();
    if min < want {
        sets.push(("minReplicas".to_owned(), json!(want)));
    }
    if spec["maxReplicas"].as_u64().is_some_and(|max| max < want) {
        sets.push(("maxReplicas".to_owned(), json!(want)));
    }
    sets
}

pub fn floor_of(spec: &Value) -> u64 {
    spec["minReplicas"].as_u64().unwrap_or(DEFAULT_FLOOR)
}

async fn values_floor(env: &Env, want: u64) -> Res<ValueGuard> {
    let mut guard = ValueGuard::new(env);
    let current = guard
        .current(&floor_key(env))
        .await?
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    if current < want {
        let max_key = floor_key(env).replace(".floor", ".max_sandboxes");
        let max = guard
            .current(&max_key)
            .await?
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let mut sets = vec![
            (floor_key(env), json!(want)),
            ("quota.extra_pods".to_owned(), json!(want + 4)),
        ];
        if max < want {
            sets.push((max_key, json!(want)));
        }
        guard.set(&sets).await?;
    }
    Ok(guard)
}

async fn pool_floor(env: &Env, want: u64) -> Res<PoolGuard> {
    let mut guard = PoolGuard::new(env);
    let sets = floor_sets(&guard.spec().await?, want);
    if !sets.is_empty() {
        guard.set(&sets).await?;
    }
    Ok(guard)
}

pub async fn ensure_floor(env: &Env, want: u64) -> Res<FloorGuard> {
    let mut floor = FloorGuard {
        _values: None,
        _pool: None,
    };
    match ChartLine::of(&env.chart_version)? {
        ChartLine::PoolResource => floor._pool = Some(pool_floor(env, want).await?),
        ChartLine::Values => floor._values = Some(values_floor(env, want).await?),
    }
    pool::wait_warm(env, want as usize, WARM_WAIT_SECONDS).await?;
    Ok(floor)
}

pub async fn current_floor(env: &Env) -> Res<usize> {
    let floor = match ChartLine::of(&env.chart_version)? {
        ChartLine::PoolResource => floor_of(&PoolGuard::new(env).spec().await?),
        ChartLine::Values => ValueGuard::new(env)
            .current(&floor_key(env))
            .await?
            .and_then(|value| value.as_u64())
            .unwrap_or(DEFAULT_FLOOR),
    };
    Ok(floor as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_floor_above_the_minimum_raises_the_minimum_only_when_the_maximum_holds_it() {
        let spec = json!({"minReplicas": 1, "maxReplicas": 5});
        assert_eq!(floor_sets(&spec, 3), [("minReplicas".to_owned(), json!(3))]);
    }

    #[test]
    fn a_maximum_below_the_floor_is_raised_in_the_same_change() {
        let spec = json!({"minReplicas": 1, "maxReplicas": 1});
        assert_eq!(
            floor_sets(&spec, 4),
            [
                ("minReplicas".to_owned(), json!(4)),
                ("maxReplicas".to_owned(), json!(4))
            ]
        );
    }

    #[test]
    fn a_pool_that_holds_the_floor_is_not_changed() {
        let spec = json!({"minReplicas": 4, "maxReplicas": 4});
        assert!(floor_sets(&spec, 3).is_empty());
    }

    #[test]
    fn a_pool_with_no_maximum_gets_no_maximum() {
        let spec = json!({"image": "x"});
        assert_eq!(floor_sets(&spec, 3), [("minReplicas".to_owned(), json!(3))]);
    }

    #[test]
    fn the_floor_of_a_spec_is_its_minimum_or_one() {
        assert_eq!(floor_of(&json!({"minReplicas": 0})), 0);
        assert_eq!(floor_of(&json!({"minReplicas": 3})), 3);
        assert_eq!(floor_of(&json!({"image": "x"})), 1);
    }
}
