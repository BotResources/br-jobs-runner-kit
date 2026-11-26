use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Readiness {
    expected: Mutex<Option<HashSet<String>>>,
    synced: Mutex<HashSet<String>>,
    store_ready: AtomicBool,
}

impl Readiness {
    #[cfg(test)]
    pub fn new(expected_namespaces: impl IntoIterator<Item = String>) -> Self {
        let readiness = Self::starting();
        readiness.expect(expected_namespaces);
        readiness
    }

    pub fn starting() -> Self {
        Self {
            expected: Mutex::new(None),
            synced: Mutex::new(HashSet::new()),
            store_ready: AtomicBool::new(false),
        }
    }

    pub fn expect(&self, namespaces: impl IntoIterator<Item = String>) {
        *self.expected.lock().unwrap() = Some(namespaces.into_iter().collect());
    }

    pub fn namespace_synced(&self, namespace: &str) {
        self.synced.lock().unwrap().insert(namespace.to_owned());
    }

    pub fn store_ready(&self) {
        self.store_ready.store(true, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        let expected = self.expected.lock().unwrap();
        self.store_ready.load(Ordering::SeqCst)
            && expected
                .as_ref()
                .is_some_and(|expected| self.synced.lock().unwrap().is_superset(expected))
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
    fn it_is_not_ready_while_the_namespaces_are_not_known() {
        let readiness = Readiness::starting();
        readiness.store_ready();
        assert!(!readiness.is_ready());
        readiness.expect([]);
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
