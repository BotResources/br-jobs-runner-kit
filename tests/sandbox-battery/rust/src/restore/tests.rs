use std::process::Command;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::Duration;

use tokio::signal::unix::SignalKind;

use super::*;

pub(crate) static SIGNAL_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) fn send_signal(signal_name: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal_name}"), &std::process::id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

fn quiet() -> Arc<Registry> {
    Registry::new(|_| {})
}

fn counting(registry: &Arc<Registry>, what: &str) -> (Registration, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let witness = Arc::clone(&count);
    let registration = registry.register(what, &std::env::temp_dir(), move || {
        witness.fetch_add(1, Ordering::SeqCst);
        Vec::new()
    });
    (registration, count)
}

fn deliver(kind: SignalKind, signal_name: &str) -> (i32, usize) {
    let _serial = SIGNAL_TESTS.blocking_lock();
    let (codes, received) = mpsc::channel();
    let registry = Registry::new(move |code| {
        let _ = codes.send(code);
    });
    let (registration, count) = counting(&registry, "restore-test-signal");
    let listening = registry.listen(&[kind]);
    listening
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    send_signal(signal_name);
    let code = received.recv_timeout(Duration::from_secs(10)).unwrap();
    let runs = count.load(Ordering::SeqCst);
    drop(registration);
    (code, runs)
}

#[test]
fn a_term_signal_runs_the_registered_restore_once_and_exits_with_143() {
    assert_eq!(deliver(SignalKind::terminate(), "TERM"), (143, 1));
}

#[test]
fn an_interrupt_signal_runs_the_registered_restore_once_and_exits_with_130() {
    assert_eq!(deliver(SignalKind::interrupt(), "INT"), (130, 1));
}

#[test]
fn a_restore_runs_once_when_the_guard_and_the_signal_both_ask_for_it() {
    let registry = quiet();
    let (registration, count) = counting(&registry, "restore-test-once");
    assert!(registry.is_registered("restore-test-once"));
    assert!(registration.restore().is_empty());
    registry.restore_all();
    assert!(registration.restore().is_empty());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn a_dropped_registration_is_not_restored_by_a_later_signal() {
    let registry = quiet();
    let (registration, count) = counting(&registry, "restore-test-dropped");
    drop(registration);
    assert!(!registry.is_registered("restore-test-dropped"));
    registry.restore_all();
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn a_registration_of_another_registry_is_not_touched() {
    let first = quiet();
    let second = quiet();
    let (_kept, count) = counting(&first, "restore-test-first");
    second.restore_all();
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn a_failed_restore_is_returned_to_the_caller() {
    let entry = Entry {
        id: u64::MAX,
        what: "restore-test-failed".to_owned(),
        evidence: std::env::temp_dir(),
        action: Box::new(|| vec!["helm exit 1".to_owned()]),
        done: Mutex::new(false),
    };
    assert_eq!(entry.run_once(), Some(vec!["helm exit 1".to_owned()]));
    assert_eq!(entry.run_once(), None);
}

#[test]
fn the_restore_waits_for_the_work_that_runs_and_no_work_starts_after_it() {
    let registry = quiet();
    let (_registration, count) = counting(&registry, "restore-test-work");
    let work = registry.work().unwrap();
    let waiting = Arc::clone(&registry);
    let restore = std::thread::spawn(move || waiting.restore_all());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(work);
    restore.join().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(matches!(registry.work(), Err(BatteryError::Stopping)));
}
