use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::{Notify, watch};

use super::GateMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub mode: GateMode,
    pub epoch: u64,
}

pub struct State {
    setting: watch::Sender<Setting>,
    bound: Mutex<HashMap<u64, u64>>,
    next: AtomicU64,
    accepted: AtomicU64,
    moved: Notify,
}

impl State {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            setting: watch::channel(Setting {
                mode: GateMode::Open,
                epoch: 0,
            })
            .0,
            bound: Mutex::new(HashMap::new()),
            next: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            moved: Notify::new(),
        })
    }

    fn bound(&self) -> std::sync::MutexGuard<'_, HashMap<u64, u64>> {
        self.bound.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn apply(&self, mode: GateMode) {
        self.setting.send_modify(|setting| {
            setting.mode = mode;
            setting.epoch += 1;
        });
    }

    pub fn current(&self) -> Setting {
        *self.setting.borrow()
    }

    pub fn count_accepted(&self) {
        self.accepted.fetch_add(1, Ordering::SeqCst);
    }

    pub fn accepted(&self) -> u64 {
        self.accepted.load(Ordering::SeqCst)
    }

    pub fn live(&self) -> usize {
        self.bound().len()
    }

    pub fn behind(&self) -> usize {
        let epoch = self.current().epoch;
        self.bound()
            .values()
            .filter(|bound| **bound != epoch)
            .count()
    }

    pub fn enter(self: &Arc<Self>) -> (Slot, Setting) {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let setting = {
            let mut bound = self.bound();
            let setting = self.current();
            bound.insert(id, setting.epoch);
            setting
        };
        let slot = Slot {
            state: Arc::clone(self),
            id,
            epoch: AtomicU64::new(setting.epoch),
            watcher: self.setting.subscribe(),
        };
        (slot, setting)
    }

    pub async fn settled(&self, limit: Duration) -> bool {
        let wait = async {
            while self.behind() > 0 {
                tokio::select! {
                    () = self.moved.notified() => {}
                    () = tokio::time::sleep(Duration::from_millis(5)) => {}
                }
            }
        };
        tokio::time::timeout(limit, wait).await.is_ok()
    }
}

pub struct Slot {
    state: Arc<State>,
    id: u64,
    epoch: AtomicU64,
    watcher: watch::Receiver<Setting>,
}

impl Slot {
    pub fn rebind(&self) {
        let mut bound = self.state.bound();
        let setting = self.state.current();
        self.epoch.store(setting.epoch, Ordering::SeqCst);
        bound.insert(self.id, setting.epoch);
        drop(bound);
        self.state.moved.notify_waiters();
    }

    pub async fn until_behind(&self) {
        let mut watcher = self.watcher.clone();
        loop {
            if watcher.borrow_and_update().epoch != self.epoch.load(Ordering::SeqCst) {
                return;
            }
            if watcher.changed().await.is_err() {
                return;
            }
        }
    }

    pub async fn while_current<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            output = work => Some(output),
            () = self.until_behind() => None,
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.state.bound().remove(&self.id);
        self.state.moved.notify_waiters();
    }
}
