use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_jobs::{Arrival, Error};

use super::{ArrivalOpener, ArrivalSink, ArrivalSource, keep};
use crate::instances::Instances;
use crate::shared::Shared;
use crate::unit_support::{id, shared};

#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Arrival(Arrival),
    Ended(Error),
    OpenAgain,
}

#[derive(Default)]
struct Sink {
    seen: Mutex<Vec<Seen>>,
    panic_on_arrival: AtomicBool,
}

impl Sink {
    fn seen(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn note(&self, seen: Seen) {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(seen);
    }
}

impl ArrivalSink for Sink {
    fn arrival(&self, arrival: &Arrival) {
        if self.panic_on_arrival.swap(false, Ordering::SeqCst) {
            panic!("the book panics");
        }
        self.note(Seen::Arrival(arrival.clone()));
    }

    fn arrival_ended(&self, error: &Error) {
        self.note(Seen::Ended(error.clone()));
    }

    fn arrival_open_again(&self) {
        self.note(Seen::OpenAgain);
    }
}

struct Feed(VecDeque<Result<Arrival, Error>>);

impl ArrivalSource for Feed {
    async fn next(&mut self) -> Result<Arrival, Error> {
        match self.0.pop_front() {
            Some(next) => next,
            None => std::future::pending().await,
        }
    }
}

struct Opener {
    script: Mutex<VecDeque<Result<Feed, Error>>>,
    opened: Arc<AtomicUsize>,
}

impl ArrivalOpener for Opener {
    type Source = Feed;

    async fn open(&self) -> Result<Feed, Error> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        let next = self
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        match next {
            Some(next) => next,
            None => std::future::pending().await,
        }
    }
}

struct Rig {
    shared: Arc<Shared>,
    sink: Arc<Sink>,
    opened: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

fn feed(items: Vec<Result<Arrival, Error>>) -> Result<Feed, Error> {
    Ok(Feed(items.into()))
}

fn unreadable(child: Option<u128>) -> Arrival {
    Arrival::Unreadable {
        child_job_id: child.map(id),
        error: Error::Unreadable("a bad item".to_owned()),
    }
}

fn cut() -> Error {
    Error::InvalidInput("the feed was cut".to_owned())
}

fn start(script: Vec<Result<Feed, Error>>, sink: Sink) -> Rig {
    let jobs = ScriptedJobs::new("analyst", "i1");
    let shared = shared(&jobs, None, &Arc::new(Instances::default()));
    let sink = Arc::new(sink);
    let opened = Arc::new(AtomicUsize::new(0));
    let opener = Opener {
        script: Mutex::new(script.into()),
        opened: Arc::clone(&opened),
    };
    let task = tokio::spawn(keep(Arc::clone(&shared), id(1), opener, Arc::clone(&sink)));
    Rig {
        shared,
        sink,
        opened,
        task,
    }
}

async fn settle() {
    tokio::time::sleep(Duration::from_secs(3600)).await;
}

impl Rig {
    fn opened(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }

    async fn beat(&self) {
        self.shared.beat_now();
        settle().await;
    }
}

#[tokio::test(start_paused = true)]
async fn given_a_feed_with_events_when_they_arrive_then_the_book_gets_them_in_order_and_no_reopen_is_told()
 {
    let rig = start(
        vec![feed(vec![
            Ok(unreadable(Some(7))),
            Ok(unreadable(None)),
            Ok(unreadable(Some(8))),
        ])],
        Sink::default(),
    );
    settle().await;
    assert_eq!(
        rig.sink.seen(),
        vec![
            Seen::Arrival(unreadable(Some(7))),
            Seen::Arrival(unreadable(None)),
            Seen::Arrival(unreadable(Some(8))),
        ]
    );
    assert_eq!(rig.opened(), 1);
    rig.task.abort();
}

#[tokio::test(start_paused = true)]
async fn given_a_feed_that_ends_with_an_error_when_no_beat_comes_then_the_book_gets_the_end_and_no_open_is_tried()
 {
    let rig = start(
        vec![feed(vec![Ok(unreadable(None)), Err(cut())])],
        Sink::default(),
    );
    settle().await;
    assert_eq!(
        rig.sink.seen(),
        vec![Seen::Arrival(unreadable(None)), Seen::Ended(cut())]
    );
    assert_eq!(rig.opened(), 1);
    rig.task.abort();
}

#[tokio::test(start_paused = true)]
async fn given_a_feed_that_ended_when_the_beat_comes_then_it_is_opened_again_and_the_book_is_told_once()
 {
    let rig = start(
        vec![feed(vec![Err(cut())]), feed(vec![Ok(unreadable(Some(3)))])],
        Sink::default(),
    );
    settle().await;
    rig.beat().await;
    assert_eq!(
        rig.sink.seen(),
        vec![
            Seen::Ended(cut()),
            Seen::OpenAgain,
            Seen::Arrival(unreadable(Some(3)))
        ]
    );
    assert_eq!(rig.opened(), 2);
    rig.task.abort();
}

#[tokio::test(start_paused = true)]
async fn given_a_feed_that_cannot_be_opened_when_no_beat_comes_then_the_book_gets_the_end_and_the_open_is_not_tried_in_a_loop()
 {
    let rig = start(vec![Err(cut()), feed(vec![])], Sink::default());
    settle().await;
    assert_eq!(rig.sink.seen(), vec![Seen::Ended(cut())]);
    assert_eq!(rig.opened(), 1);
    rig.beat().await;
    assert_eq!(rig.sink.seen(), vec![Seen::Ended(cut()), Seen::OpenAgain]);
    assert_eq!(rig.opened(), 2);
    rig.task.abort();
}

#[tokio::test(start_paused = true)]
async fn given_a_first_open_that_works_when_events_arrive_then_the_book_is_not_told_of_a_reopen() {
    let rig = start(vec![feed(vec![Ok(unreadable(None))])], Sink::default());
    settle().await;
    assert!(!rig.sink.seen().contains(&Seen::OpenAgain));
    rig.task.abort();
}

#[tokio::test(start_paused = true)]
async fn given_a_book_that_panics_on_an_event_then_the_end_is_given_to_the_book_and_the_feed_is_opened_again_after_the_beat()
 {
    let sink = Sink::default();
    sink.panic_on_arrival.store(true, Ordering::SeqCst);
    let rig = start(
        vec![
            feed(vec![Ok(unreadable(Some(1)))]),
            feed(vec![Ok(unreadable(Some(2)))]),
        ],
        sink,
    );
    settle().await;
    let seen = rig.sink.seen();
    assert_eq!(seen.len(), 1);
    assert!(
        matches!(&seen[0], Seen::Ended(Error::InvalidInput(text)) if text.contains("the book panics")),
        "{seen:?}"
    );
    assert_eq!(rig.opened(), 1);
    rig.beat().await;
    let seen = rig.sink.seen();
    assert_eq!(seen[1], Seen::OpenAgain);
    assert_eq!(seen[2], Seen::Arrival(unreadable(Some(2))));
    assert_eq!(rig.opened(), 2);
    rig.task.abort();
}
