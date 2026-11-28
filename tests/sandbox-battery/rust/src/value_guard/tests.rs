use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::*;
use crate::restore::tests::{SIGNAL_TESTS, send_signal};

struct FakeHelm {
    log: Arc<Mutex<Vec<String>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}

impl FakeHelm {
    fn new(log: &Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            log: Arc::clone(log),
            release: Mutex::new(None),
        }
    }

    fn held_by(log: &Arc<Mutex<Vec<String>>>) -> (Self, oneshot::Sender<()>) {
        let (open, held) = oneshot::channel();
        let fake = Self::new(log);
        *fake.release.lock().unwrap() = Some(held);
        (fake, open)
    }

    fn note(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }
}

impl HelmCalls for FakeHelm {
    async fn values(&self) -> Res<Value> {
        Ok(json!({}))
    }

    async fn supplied_values(&self) -> Res<Value> {
        self.note("supplied".to_owned());
        Ok(json!({"pool": {"size": 3}}))
    }

    async fn upgrade(&self, sets: &[(String, Value)]) -> Res {
        self.note(format!("upgrade-start {}", sets[0].0));
        let held = self.release.lock().unwrap().take();
        if let Some(held) = held {
            let _ = held.await;
        }
        self.note(format!("upgrade-end {}", sets[0].0));
        Ok(())
    }

    fn reset(&self, snapshot: &Value) -> Vec<String> {
        self.note(format!("reset {snapshot}"));
        Vec::new()
    }
}

fn sets(key: &str) -> Vec<(String, Value)> {
    vec![(key.to_owned(), json!(1))]
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

#[tokio::test]
async fn set_records_the_previous_values_once_and_the_drop_restores_them_once() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, _codes) = exiting();
    let mut guard = ValueGuard::with(
        FakeHelm::new(&log),
        std::env::temp_dir(),
        Arc::clone(&registry),
    );
    guard.set(&sets("a")).await.unwrap();
    guard.set(&sets("b")).await.unwrap();
    drop(guard);
    registry.restore_all();
    assert_eq!(
        lines(&log),
        [
            "supplied",
            "upgrade-start a",
            "upgrade-end a",
            "upgrade-start b",
            "upgrade-end b",
            "reset {\"pool\":{\"size\":3}}"
        ]
    );
}

#[tokio::test]
async fn a_term_signal_after_set_restores_once_and_exits_with_143() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, codes) = exiting();
    let mut guard = ValueGuard::with(
        FakeHelm::new(&log),
        std::env::temp_dir(),
        Arc::clone(&registry),
    );
    guard.set(&sets("a")).await.unwrap();
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
async fn a_signal_during_set_waits_for_the_running_upgrade_before_the_restore() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, codes) = exiting();
    let (fake, open) = FakeHelm::held_by(&log);
    let mut guard = ValueGuard::with(fake, std::env::temp_dir(), Arc::clone(&registry));
    let watched = Arc::clone(&log);
    let driver = async move {
        while !lines(&watched)
            .iter()
            .any(|line| line.starts_with("upgrade-start"))
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        send_signal("TERM");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            !lines(&watched).iter().any(|line| line.starts_with("reset")),
            "the restore started while the upgrade was running"
        );
        assert!(codes.try_recv().is_err());
        open.send(()).unwrap();
        tokio::task::spawn_blocking(move || codes.recv_timeout(Duration::from_secs(10)))
            .await
            .unwrap()
            .unwrap()
    };
    let values = sets("a");
    let (set_result, code) = tokio::join!(guard.set(&values), driver);
    set_result.unwrap();
    assert_eq!(code, 143);
    drop(guard);
    assert_eq!(
        lines(&log),
        [
            "supplied",
            "upgrade-start a",
            "upgrade-end a",
            "reset {\"pool\":{\"size\":3}}"
        ]
    );
}

#[tokio::test]
async fn no_set_starts_once_the_restore_has_begun() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (registry, _codes) = exiting();
    let mut guard = ValueGuard::with(
        FakeHelm::new(&log),
        std::env::temp_dir(),
        Arc::clone(&registry),
    );
    registry.restore_all();
    assert!(matches!(
        guard.set(&sets("a")).await,
        Err(crate::error::BatteryError::Stopping)
    ));
    assert!(lines(&log).is_empty());
}
