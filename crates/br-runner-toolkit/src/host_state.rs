use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::watch;

use crate::tap::Tap;

pub(crate) struct HostState {
    pub cancel: watch::Sender<bool>,
    pub finished: watch::Sender<usize>,
    pub tap: Arc<Tap>,
    taken: AtomicUsize,
}

impl HostState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            cancel: watch::channel(false).0,
            finished: watch::channel(0).0,
            tap: Arc::new(Tap::default()),
            taken: AtomicUsize::new(0),
        })
    }

    pub(crate) fn run_taken(&self) {
        self.taken.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn has_taken_a_run(&self) -> bool {
        self.taken.load(Ordering::SeqCst) > 0
    }

    pub(crate) fn run_forgotten(&self) {
        self.finished.send_modify(|count| *count += 1);
    }

    pub(crate) fn cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
}
