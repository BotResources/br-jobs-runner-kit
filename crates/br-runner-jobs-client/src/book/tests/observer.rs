use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::book::AgentKey;
use crate::queue::{QueueObserver, QueueReport};

struct ReadsTheBook {
    book: Mutex<Option<Arc<AgentBook>>>,
    reads: AtomicUsize,
}

impl QueueObserver for ReadsTheBook {
    fn report(&self, _report: &QueueReport) {
        let book = self.book.lock().expect("an observer").clone();
        if let Some(book) = book {
            let _ = book.agents();
            self.reads.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn given_a_stopped_queue_when_an_agent_is_added_then_the_observer_runs_after_the_book_is_free_to_read()
 {
    let observer = Arc::new(ReadsTheBook {
        book: Mutex::new(None),
        reads: AtomicUsize::new(0),
    });
    let jobs = scripted();
    let work = work(&jobs);
    let mut queue_settings = settings();
    queue_settings.observer = observer.clone();
    let (queue, _started) = work.delivery_queue(queue_settings).await.expect("a queue");
    let book = Arc::new(work.agent_book(&queue, Vec::new(), false).expect("a book"));
    *observer.book.lock().expect("an observer") = Some(Arc::clone(&book));
    drop(queue);
    let (done, finished) = std::sync::mpsc::channel();
    let worker = {
        let book = Arc::clone(&book);
        std::thread::spawn(move || {
            let added = book.agent_for(&AgentKey::new("node", "label"), true);
            let _ = done.send(added.agent_id);
        })
    };
    let agent_id = finished
        .recv_timeout(Duration::from_secs(5))
        .expect("adding an agent does not wait for the observer");
    worker.join().expect("the thread ends");
    assert!(observer.reads.load(Ordering::SeqCst) > 0);
    assert_eq!(book.agents().len(), 1);
    assert_eq!(book.agents()[0].agent_id, agent_id);
}
