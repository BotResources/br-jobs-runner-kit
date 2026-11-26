use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Default)]
pub struct Gate {
    patches: Arc<AtomicBool>,
    creates: Arc<AtomicBool>,
}

impl Gate {
    pub fn park_after_patch(&self) {
        self.patches.store(true, Ordering::SeqCst);
    }

    pub fn park_after_create(&self) {
        self.creates.store(true, Ordering::SeqCst);
    }

    pub fn open(&self) {
        self.patches.store(false, Ordering::SeqCst);
        self.creates.store(false, Ordering::SeqCst);
    }

    pub async fn after_patch(&self) {
        if self.patches.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
    }

    pub async fn after_create(&self) {
        if self.creates.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
    }
}
