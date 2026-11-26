use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Readiness {
    expected: HashSet<String>,
    synced: Mutex<HashSet<String>>,
    store_ready: AtomicBool,
}

impl Readiness {
    pub fn new(expected_namespaces: impl IntoIterator<Item = String>) -> Self {
        Self {
            expected: expected_namespaces.into_iter().collect(),
            synced: Mutex::new(HashSet::new()),
            store_ready: AtomicBool::new(false),
        }
    }

    pub fn namespace_synced(&self, namespace: &str) {
        self.synced.lock().unwrap().insert(namespace.to_owned());
    }

    pub fn store_ready(&self) {
        self.store_ready.store(true, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.store_ready.load(Ordering::SeqCst) && self.synced.lock().unwrap().is_superset(&self.expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readiness() -> Readiness {
        Readiness::new(["a".to_owned(), "b".to_owned()])
    }

    #[test]
    fn it_is_not_ready_until_every_watch_has_synced_and_the_store_is_good() {
        let readiness = readiness();
        assert!(!readiness.is_ready());
        readiness.namespace_synced("a");
        readiness.store_ready();
        assert!(!readiness.is_ready());
        readiness.namespace_synced("b");
        assert!(readiness.is_ready());
    }

    #[test]
    fn synced_watches_alone_do_not_make_it_ready() {
        let readiness = readiness();
        readiness.namespace_synced("a");
        readiness.namespace_synced("b");
        assert!(!readiness.is_ready());
    }
}
