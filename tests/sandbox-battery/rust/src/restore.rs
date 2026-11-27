use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use tokio::signal::unix::{SignalKind, signal};

use crate::guards::report_failure;

type Action = Box<dyn Fn() -> Vec<String> + Send + Sync>;

struct Entry {
    id: u64,
    what: String,
    evidence: PathBuf,
    action: Action,
    done: Mutex<bool>,
}

static ENTRIES: Mutex<Vec<Arc<Entry>>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(0);
static LISTENER: OnceLock<()> = OnceLock::new();

fn entries() -> std::sync::MutexGuard<'static, Vec<Arc<Entry>>> {
    ENTRIES.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Entry {
    fn run_once(&self) -> Option<Vec<String>> {
        let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        if *done {
            return None;
        }
        *done = true;
        Some((self.action)())
    }
}

pub struct Registration {
    entry: Arc<Entry>,
}

impl Registration {
    pub fn restore(&self) -> Vec<String> {
        self.entry.run_once().unwrap_or_default()
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let id = self.entry.id;
        entries().retain(|entry| entry.id != id);
    }
}

pub fn register(
    what: &str,
    evidence: &Path,
    action: impl Fn() -> Vec<String> + Send + Sync + 'static,
) -> Registration {
    let entry = Arc::new(Entry {
        id: NEXT.fetch_add(1, Ordering::SeqCst),
        what: what.to_owned(),
        evidence: evidence.to_path_buf(),
        action: Box::new(action),
        done: Mutex::new(false),
    });
    entries().push(Arc::clone(&entry));
    Registration { entry }
}

pub fn is_registered(what: &str) -> bool {
    entries().iter().any(|entry| entry.what == what)
}

pub fn restore_all() {
    let all: Vec<Arc<Entry>> = entries().clone();
    for entry in all {
        for detail in entry.run_once().unwrap_or_default() {
            report_failure(&entry.evidence, &entry.what, &detail);
        }
    }
}

pub fn listen(kinds: &[SignalKind], done: impl Fn(i32) + Send + 'static) -> Receiver<()> {
    let (ready, listening) = mpsc::channel();
    let kinds = kinds.to_vec();
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            eprintln!("restore on signal: the signal thread has no runtime");
            return;
        };
        runtime.block_on(async move {
            let mut streams = Vec::new();
            for kind in kinds {
                match signal(kind) {
                    Ok(stream) => streams.push((kind, stream)),
                    Err(error) => {
                        eprintln!("restore on signal: no handler for {kind:?}: {error}");
                        return;
                    }
                }
            }
            let _ = ready.send(());
            let received = futures::future::select_all(streams.iter_mut().map(|(kind, stream)| {
                Box::pin(async move {
                    stream.recv().await;
                    *kind
                })
            }))
            .await
            .0;
            restore_all();
            done(exit_code(received));
        });
    });
    listening
}

pub fn exit_code(kind: SignalKind) -> i32 {
    if kind == SignalKind::interrupt() {
        130
    } else {
        143
    }
}

pub fn listen_once() {
    LISTENER.get_or_init(|| {
        drop(listen(
            &[SignalKind::terminate(), SignalKind::interrupt()],
            |code| std::process::exit(code),
        ));
    });
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;

    fn counting(what: &str) -> (Registration, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let witness = Arc::clone(&count);
        let registration = register(what, &std::env::temp_dir(), move || {
            witness.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        });
        (registration, count)
    }

    fn send(signal_name: &str) {
        let status = Command::new("kill")
            .args([&format!("-{signal_name}"), &std::process::id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn deliver(kind: SignalKind, signal_name: &str, what: &str) -> (i32, usize) {
        let (registration, count) = counting(what);
        let (codes, received) = mpsc::channel();
        let listening = listen(&[kind], move |code| {
            codes.send(code).unwrap();
        });
        listening.recv_timeout(Duration::from_secs(10)).unwrap();
        send(signal_name);
        let code = received.recv_timeout(Duration::from_secs(10)).unwrap();
        let runs = count.load(Ordering::SeqCst);
        drop(registration);
        (code, runs)
    }

    #[test]
    fn a_term_signal_runs_the_registered_restore_once_and_exits_with_143() {
        let (code, runs) = deliver(SignalKind::terminate(), "TERM", "restore-test-term");
        assert_eq!((code, runs), (143, 1));
    }

    #[test]
    fn an_interrupt_signal_runs_the_registered_restore_once_and_exits_with_130() {
        let (code, runs) = deliver(SignalKind::interrupt(), "INT", "restore-test-interrupt");
        assert_eq!((code, runs), (130, 1));
    }

    #[test]
    fn a_restore_runs_once_when_the_guard_and_the_signal_both_ask_for_it() {
        let (registration, count) = counting("restore-test-once");
        assert!(is_registered("restore-test-once"));
        assert!(registration.restore().is_empty());
        restore_all();
        assert!(registration.restore().is_empty());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_dropped_registration_is_not_restored_by_a_later_signal() {
        let (registration, count) = counting("restore-test-dropped");
        drop(registration);
        assert!(!is_registered("restore-test-dropped"));
        restore_all();
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
}
