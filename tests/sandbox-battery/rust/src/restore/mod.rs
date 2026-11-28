mod listener;
#[cfg(test)]
pub(crate) mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};

use crate::error::BatteryError;
use crate::guards::report_failure;

pub use listener::exit_code;

type Action = Box<dyn Fn() -> Vec<String> + Send + Sync>;
type Exit = Arc<dyn Fn(i32) + Send + Sync>;

static GLOBAL: OnceLock<Arc<Registry>> = OnceLock::new();

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Entry {
    id: u64,
    what: String,
    evidence: PathBuf,
    action: Action,
    done: Mutex<bool>,
}

impl Entry {
    fn run_once(&self) -> Option<Vec<String>> {
        let mut done = locked(&self.done);
        if *done {
            return None;
        }
        *done = true;
        Some((self.action)())
    }
}

#[derive(Default)]
struct Gate {
    busy: usize,
    closing: bool,
}

pub struct Registry {
    entries: Mutex<Vec<Arc<Entry>>>,
    next: AtomicU64,
    gate: Mutex<Gate>,
    idle: Condvar,
    listening: Mutex<bool>,
    exit: Exit,
}

pub struct Registration {
    entry: Arc<Entry>,
    registry: Arc<Registry>,
}

pub struct Work {
    registry: Arc<Registry>,
}

impl Registration {
    pub fn restore(&self) -> Vec<String> {
        self.entry.run_once().unwrap_or_default()
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let id = self.entry.id;
        locked(&self.registry.entries).retain(|entry| entry.id != id);
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let mut gate = locked(&self.registry.gate);
        gate.busy -= 1;
        self.registry.idle.notify_all();
    }
}

impl Registry {
    pub fn new(exit: impl Fn(i32) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(Vec::new()),
            next: AtomicU64::new(0),
            gate: Mutex::new(Gate::default()),
            idle: Condvar::new(),
            listening: Mutex::new(false),
            exit: Arc::new(exit),
        })
    }

    pub fn global() -> Arc<Self> {
        Arc::clone(GLOBAL.get_or_init(|| Self::new(|code| std::process::exit(code))))
    }

    pub fn register(
        self: &Arc<Self>,
        what: &str,
        evidence: &Path,
        action: impl Fn() -> Vec<String> + Send + Sync + 'static,
    ) -> Registration {
        let entry = Arc::new(Entry {
            id: self.next.fetch_add(1, Ordering::SeqCst),
            what: what.to_owned(),
            evidence: evidence.to_path_buf(),
            action: Box::new(action),
            done: Mutex::new(false),
        });
        locked(&self.entries).push(Arc::clone(&entry));
        Registration {
            entry,
            registry: Arc::clone(self),
        }
    }

    pub fn is_registered(&self, what: &str) -> bool {
        locked(&self.entries).iter().any(|entry| entry.what == what)
    }

    pub fn work(self: &Arc<Self>) -> Result<Work, BatteryError> {
        let mut gate = locked(&self.gate);
        if gate.closing {
            return Err(BatteryError::Stopping);
        }
        gate.busy += 1;
        Ok(Work {
            registry: Arc::clone(self),
        })
    }

    pub fn restore_all(&self) {
        {
            let mut gate = locked(&self.gate);
            gate.closing = true;
            while gate.busy > 0 {
                gate = self.idle.wait(gate).unwrap_or_else(PoisonError::into_inner);
            }
        }
        let all: Vec<Arc<Entry>> = locked(&self.entries).clone();
        for entry in all {
            for detail in entry.run_once().unwrap_or_default() {
                report_failure(&entry.evidence, &entry.what, &detail);
            }
        }
    }
}
