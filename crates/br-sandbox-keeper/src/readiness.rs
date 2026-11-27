use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::cluster::Views;

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct Readiness {
    views: Mutex<Option<Views>>,
    store_ready: AtomicBool,
}

impl Readiness {
    pub fn starting() -> Self {
        Self {
            views: Mutex::new(None),
            store_ready: AtomicBool::new(false),
        }
    }

    pub fn watch(&self, views: Views) {
        *held(&self.views) = Some(views);
    }

    pub fn store_ready(&self) {
        self.store_ready.store(true, Ordering::SeqCst);
    }

    pub fn not_ready_reason(&self) -> Option<String> {
        let views = held(&self.views).clone();
        let Some(views) = views else {
            return Some("the views are not started".to_owned());
        };
        if let Some(reason) = views.not_ready_reason() {
            return Some(reason);
        }
        (!self.store_ready.load(Ordering::SeqCst))
            .then(|| "the object store has not been checked".to_owned())
    }

    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        self.not_ready_reason().is_none()
    }
}

#[cfg(test)]
mod tests;
