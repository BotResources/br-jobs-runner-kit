use std::time::{Duration, Instant};

use serde_json::Value;

use crate::env::Env;
use crate::error::{BatteryError, Res, data};
use crate::proc::Kube;

fn pod_count(list: &Value) -> Res<usize> {
    list["items"]
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| data("the keeper pod list", "no items array"))
}

const PODS_APPEAR_SECONDS: u64 = 30;

fn no_pod_found(label: &str, namespace: &str) -> BatteryError {
    data(
        "KEEPER_LABEL",
        format!(
            "{label} matches no keeper pod in {namespace} after {PODS_APPEAR_SECONDS} s: the stop of the keeper cannot be proven"
        ),
    )
}

pub async fn require_pods(env: &Env) -> Res {
    let kube = Kube::cluster_wide(env).in_namespace(&env.system_ns);
    let start = Instant::now();
    loop {
        let list = kube.json(&["get", "pods", "-l", &env.keeper_label]).await?;
        if pod_count(&list)? > 0 {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(PODS_APPEAR_SECONDS) {
            return Err(no_pod_found(&env.keeper_label, &env.system_ns));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub async fn wait_pods_gone(env: &Env, seconds: u64) -> Res {
    let kube = Kube::cluster_wide(env).in_namespace(&env.system_ns);
    let start = Instant::now();
    loop {
        let list = kube.json(&["get", "pods", "-l", &env.keeper_label]).await?;
        if pod_count(&list)? == 0 {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            return Err(BatteryError::NotReached {
                what: format!(
                    "the keeper pods ({} in {}) to be gone",
                    env.keeper_label, env.system_ns
                ),
                seconds,
            });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_pod_that_is_shutting_down_still_counts() {
        let list = json!({"items": [{"metadata": {"deletionTimestamp": "2026-01-01T00:00:00Z"}}]});
        assert_eq!(pod_count(&list).unwrap(), 1);
        assert_eq!(pod_count(&json!({"items": []})).unwrap(), 0);
    }

    #[test]
    fn a_label_that_matches_no_pod_names_the_label_and_the_namespace() {
        let error = no_pod_found("br.sandbox/system=keeper", "sbx-system").to_string();
        assert!(error.contains("KEEPER_LABEL"), "{error}");
        assert!(error.contains("br.sandbox/system=keeper"), "{error}");
        assert!(error.contains("sbx-system"), "{error}");
    }

    #[test]
    fn a_list_with_no_items_array_is_an_error_not_an_empty_list() {
        assert!(pod_count(&json!({})).is_err());
        assert!(pod_count(&json!({"items": null})).is_err());
    }
}
