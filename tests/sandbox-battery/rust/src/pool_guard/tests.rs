use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::error::BatteryError;
use crate::restore::tests::{SIGNAL_TESTS, send_signal};

const ORIGINAL: &str = r#"{"spec":{"image":"img:1","minReplicas":1,"maxReplicas":2}}"#;

struct FakeKube {
    log: Arc<Mutex<Vec<String>>>,
    patch_fails: bool,
}

impl FakeKube {
    fn new(log: &Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            log: Arc::clone(log),
            patch_fails: false,
        }
    }

    fn note(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }
}

impl PoolCalls for FakeKube {
    async fn spec(&self) -> Res<Value> {
        self.note("spec".to_owned());
        Ok(json!({"image": "img:1", "minReplicas": 1}))
    }

    async fn patch(&self, sets: &[(String, Value)]) -> Res {
        self.note(format!("patch {}", merge_body(sets)));
        if self.patch_fails {
            Err(data("patch", "refused"))
        } else {
            Ok(())
        }
    }

    fn reset(&self, snapshot: &Value) -> Vec<String> {
        self.note(format!("reset {snapshot}"));
        Vec::new()
    }
}

fn sets(key: &str, value: u64) -> Vec<(String, Value)> {
    vec![(key.to_owned(), json!(value))]
}

fn lines(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().unwrap().clone()
}

fn exiting() -> (Arc<Registry>, mpsc::Receiver<i32>) {
    let (codes, received) = mpsc::channel();
    let registry = Registry::new(move |code| {
        let _ = codes.send(code);
    });
    (registry, received)
}

fn guard_of(fake: FakeKube, registry: &Arc<Registry>) -> PoolGuard<FakeKube> {
    PoolGuard::with(fake, std::env::temp_dir(), Arc::clone(registry))
}

#[tokio::test]
async fn set_records_the_spec_once_and_the_drop_restores_it_once() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, _codes) = exiting();
    let mut guard = guard_of(FakeKube::new(&log), &registry);
    guard.set(&sets("minReplicas", 3)).await.unwrap();
    guard.set(&sets("maxReplicas", 3)).await.unwrap();
    drop(guard);
    registry.restore_all();
    assert_eq!(
        lines(&log),
        [
            "spec",
            "patch {\"spec\":{\"minReplicas\":3}}",
            "patch {\"spec\":{\"maxReplicas\":3}}",
            "reset {\"image\":\"img:1\",\"minReplicas\":1}"
        ]
    );
}

#[tokio::test]
async fn a_patch_that_fails_is_still_restored_on_drop() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, _codes) = exiting();
    let mut fake = FakeKube::new(&log);
    fake.patch_fails = true;
    let mut guard = guard_of(fake, &registry);
    assert!(guard.set(&sets("minReplicas", 3)).await.is_err());
    drop(guard);
    let resets = lines(&log)
        .iter()
        .filter(|line| line.starts_with("reset"))
        .count();
    assert_eq!(resets, 1);
}

#[tokio::test]
async fn a_guard_that_set_nothing_restores_nothing() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, _codes) = exiting();
    let guard = guard_of(FakeKube::new(&log), &registry);
    assert!(guard.spec().await.is_ok());
    drop(guard);
    assert_eq!(lines(&log), ["spec"]);
}

#[tokio::test]
async fn a_term_signal_after_set_restores_once_and_exits_with_143() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, codes) = exiting();
    let mut guard = guard_of(FakeKube::new(&log), &registry);
    guard.set(&sets("minReplicas", 3)).await.unwrap();
    send_signal("TERM");
    let code = tokio::task::spawn_blocking(move || codes.recv_timeout(Duration::from_secs(10)))
        .await
        .unwrap()
        .unwrap();
    drop(guard);
    assert_eq!(code, 143);
    let resets = lines(&log)
        .iter()
        .filter(|line| line.starts_with("reset"))
        .count();
    assert_eq!(resets, 1);
}

#[tokio::test]
async fn no_set_starts_once_the_restore_has_begun() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, _codes) = exiting();
    let mut guard = guard_of(FakeKube::new(&log), &registry);
    registry.restore_all();
    assert!(matches!(
        guard.set(&sets("minReplicas", 3)).await,
        Err(BatteryError::Stopping)
    ));
    assert!(lines(&log).is_empty());
}

fn fake_kubectl(name: &str, fail_reset: bool) -> (PathBuf, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("battery-pool-guard-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("calls.log");
    let _ = std::fs::remove_file(&log);
    let program = dir.join("kubectl");
    let failure = if fail_reset {
        "case \" $* \" in *\" --type json \"*) echo refused >&2; exit 1;; esac"
    } else {
        ""
    };
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> '{}'\n{failure}\ncase \" $* \" in *\" get \"*) echo '{ORIGINAL}';; esac\n",
        log.display()
    );
    std::fs::write(&program, script).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    (program, log)
}

fn calls(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn kube_in(program: &Path, namespace: &str) -> PoolKube {
    PoolKube::with(
        program.to_str().unwrap(),
        "ctx".to_owned(),
        namespace.to_owned(),
        "pool".to_owned(),
    )
}

#[tokio::test]
async fn only_the_named_pool_in_the_named_namespace_is_read_and_changed() {
    let _serial = SIGNAL_TESTS.lock().await;
    let (program, log) = fake_kubectl("namespace", false);
    let (registry, _codes) = exiting();
    let mut guard = PoolGuard::with(
        kube_in(&program, "scratch-ns"),
        std::env::temp_dir(),
        Arc::clone(&registry),
    );
    guard.set(&sets("minReplicas", 3)).await.unwrap();
    drop(guard);
    let all = calls(&log);
    assert_eq!(all.len(), 3, "{all:?}");
    let target = "--context ctx -n scratch-ns";
    let kind = format!("{KIND} pool");
    for line in &all {
        assert!(line.starts_with(target), "{line}");
        assert!(line.contains(&kind), "{line}");
        for forbidden in [" -A", "--all-namespaces", " delete", " apply", " create"] {
            assert!(!line.contains(forbidden), "{line}");
        }
    }
    assert!(all[0].contains(" get "));
    assert!(all[1].contains("--type merge") && all[1].ends_with("{\"spec\":{\"minReplicas\":3}}"));
    assert!(all[2].contains("--type json"));
    assert!(all[2].contains("\"path\":\"/spec\""));
    assert!(all[2].contains("\"minReplicas\":1") && all[2].contains("\"image\":\"img:1\""));
}

#[tokio::test]
async fn a_pool_that_cannot_be_read_is_an_error_and_nothing_is_changed() {
    let _serial = SIGNAL_TESTS.lock().await;
    let dir =
        std::env::temp_dir().join(format!("battery-pool-guard-absent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("kubectl");
    std::fs::write(&program, "#!/bin/sh\necho NotFound >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (registry, _codes) = exiting();
    let mut guard = PoolGuard::with(
        kube_in(&program, "scratch-ns"),
        std::env::temp_dir(),
        Arc::clone(&registry),
    );
    let error = guard.set(&sets("minReplicas", 3)).await.unwrap_err();
    assert!(error.to_string().contains("NotFound"), "{error}");
}

#[test]
fn a_failed_reset_returns_the_detail_of_the_command() {
    let (program, log) = fake_kubectl("reset-fails", true);
    let failures = kube_in(&program, "scratch-ns").reset(&json!({"image": "img:1"}));
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("refused"), "{failures:?}");
    assert_eq!(calls(&log).len(), 1);
}

#[test]
fn the_merge_body_holds_only_the_keys_that_are_set() {
    let body = merge_body(&[
        ("minReplicas".to_owned(), json!(3)),
        ("image".to_owned(), json!("img:2")),
    ]);
    assert_eq!(body, json!({"spec": {"minReplicas": 3, "image": "img:2"}}));
}

#[test]
fn the_reset_body_replaces_the_whole_spec_with_the_snapshot() {
    let body = reset_body(&json!({"image": "img:1"}));
    assert_eq!(
        body,
        json!([{"op": "replace", "path": "/spec", "value": {"image": "img:1"}}])
    );
}
