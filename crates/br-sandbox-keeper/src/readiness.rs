use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

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
        *held(&self.expected) = Some(namespaces.into_iter().collect());
    }

    pub fn namespace_synced(&self, namespace: &str) {
        held(&self.synced).insert(namespace.to_owned());
    }

    pub fn store_ready(&self) {
        self.store_ready.store(true, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        let expected = held(&self.expected);
        self.store_ready.load(Ordering::SeqCst)
            && expected
                .as_ref()
                .is_some_and(|expected| held(&self.synced).is_superset(expected))
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
    fn a_panic_while_a_lock_is_held_does_not_take_the_answer_down() {
        let readiness = std::sync::Arc::new(readiness());
        let poisoner = readiness.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.synced.lock().unwrap();
            let _other = poisoner.expected.lock().unwrap();
            panic!("a panic that poisons both locks");
        })
        .join();
        readiness.namespace_synced("a");
        readiness.namespace_synced("b");
        readiness.store_ready();
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
