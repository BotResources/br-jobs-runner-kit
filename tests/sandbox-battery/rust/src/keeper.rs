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

fn no_pod_found(label: &str, namespace: &str, seconds: u64) -> BatteryError {
    data(
        "KEEPER_LABEL",
        format!(
            "{label} matches no keeper pod in {namespace} after {seconds} s: the stop of the keeper cannot be proven"
        ),
    )
}

pub struct KeeperPods {
    kube: Kube,
    label: String,
    namespace: String,
}

impl KeeperPods {
    pub fn of(env: &Env) -> Self {
        Self::with(
            Kube::cluster_wide(env).in_namespace(&env.system_ns),
            &env.keeper_label,
            &env.system_ns,
        )
    }

    pub fn with(kube: Kube, label: &str, namespace: &str) -> Self {
        Self {
            kube,
            label: label.to_owned(),
            namespace: namespace.to_owned(),
        }
    }

    async fn count(&self) -> Res<usize> {
        pod_count(&self.kube.json(&["get", "pods", "-l", &self.label]).await?)
    }

    pub async fn require(&self, seconds: u64) -> Res {
        let start = Instant::now();
        loop {
            if self.count().await? > 0 {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(seconds) {
                return Err(no_pod_found(&self.label, &self.namespace, seconds));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn wait_gone(&self, seconds: u64) -> Res {
        let start = Instant::now();
        loop {
            if self.count().await? == 0 {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(seconds) {
                return Err(BatteryError::NotReached {
                    what: format!(
                        "the keeper pods ({} in {}) to be gone",
                        self.label, self.namespace
                    ),
                    seconds,
                });
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

pub async fn require_pods(env: &Env) -> Res {
    KeeperPods::of(env).require(PODS_APPEAR_SECONDS).await
}

pub async fn wait_pods_gone(env: &Env, seconds: u64) -> Res {
    KeeperPods::of(env).wait_gone(seconds).await
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

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
        let error = no_pod_found("br.sandbox/system=keeper", "sbx-system", 30).to_string();
        assert!(error.contains("KEEPER_LABEL"), "{error}");
        assert!(error.contains("br.sandbox/system=keeper"), "{error}");
        assert!(error.contains("sbx-system"), "{error}");
    }

    #[test]
    fn a_list_with_no_items_array_is_an_error_not_an_empty_list() {
        assert!(pod_count(&json!({})).is_err());
        assert!(pod_count(&json!({"items": null})).is_err());
    }

    fn fake_kubectl(name: &str, script: &str) -> (Kube, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("battery-keeper-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let calls = dir.join("calls.log");
        let _ = std::fs::remove_file(&calls);
        let program = dir.join("kubectl");
        let text = format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\n{script}\n",
            calls.display()
        );
        std::fs::write(&program, text).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        (
            Kube::using(program.to_str().unwrap(), "ctx", Some("sbx-system")),
            calls,
        )
    }

    fn pods(kube: Kube) -> KeeperPods {
        KeeperPods::with(kube, "app=keeper", "sbx-system")
    }

    const ONE_POD: &str = "echo '{\"items\":[{\"metadata\":{\"name\":\"keeper-1\"}}]}'";
    const NO_POD: &str = "echo '{\"items\":[]}'";

    fn call_count(calls: &Path) -> usize {
        std::fs::read_to_string(calls).unwrap().lines().count()
    }

    #[tokio::test]
    async fn the_wait_is_done_at_once_when_no_keeper_pod_exists() {
        let (kube, calls) = fake_kubectl("none", NO_POD);
        pods(kube).wait_gone(5).await.unwrap();
        assert_eq!(call_count(&calls), 1);
        let line = std::fs::read_to_string(&calls).unwrap();
        assert!(
            line.contains("--context ctx -n sbx-system") && line.contains("-l app=keeper"),
            "{line}"
        );
    }

    #[tokio::test]
    async fn the_wait_goes_on_while_a_pod_is_left_and_ends_when_it_is_gone() {
        let counter =
            std::env::temp_dir().join(format!("battery-keeper-count-{}", std::process::id()));
        let _ = std::fs::remove_file(&counter);
        let script = format!(
            "echo x >> '{c}'\nif [ \"$(wc -l < '{c}')\" -lt 3 ]; then {ONE_POD}; else {NO_POD}; fi",
            c = counter.display()
        );
        let (kube, calls) = fake_kubectl("leaves", &script);
        pods(kube).wait_gone(10).await.unwrap();
        assert_eq!(call_count(&calls), 3);
    }

    #[tokio::test]
    async fn a_pod_that_stays_fails_with_the_label_the_namespace_and_the_seconds() {
        let (kube, _calls) = fake_kubectl("stays", ONE_POD);
        let error = pods(kube).wait_gone(0).await.unwrap_err();
        assert!(matches!(error, BatteryError::NotReached { seconds: 0, .. }));
        let text = error.to_string();
        assert!(
            text.contains("app=keeper") && text.contains("sbx-system") && text.contains("gone"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn a_label_that_matches_no_pod_at_the_start_fails_with_the_label_and_the_namespace() {
        let (kube, calls) = fake_kubectl("absent", NO_POD);
        let error = pods(kube).require(0).await.unwrap_err().to_string();
        assert!(
            error.contains("KEEPER_LABEL")
                && error.contains("app=keeper")
                && error.contains("sbx-system")
                && error.contains("after 0 s"),
            "{error}"
        );
        assert!(call_count(&calls) >= 1);
    }

    #[tokio::test]
    async fn a_pod_that_exists_at_the_start_satisfies_the_requirement() {
        let (kube, calls) = fake_kubectl("present", ONE_POD);
        pods(kube).require(0).await.unwrap();
        assert_eq!(call_count(&calls), 1);
    }

    #[tokio::test]
    async fn a_failing_kubectl_is_an_error_and_not_an_empty_list() {
        let (kube, _calls) = fake_kubectl("fails", "echo refused >&2; exit 1");
        let error = pods(kube).wait_gone(5).await.unwrap_err().to_string();
        assert!(error.contains("refused"), "{error}");
    }
}
