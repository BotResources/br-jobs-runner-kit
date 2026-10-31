use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::FutureExt;

use super::support::*;
use crate::queue::{DeliveryQueue, Gate, QueueObserver, QueueReport, Write, WriteKind};

struct Blocking {
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl QueueObserver for Blocking {
    fn report(&self, report: &QueueReport) {
        if matches!(report, QueueReport::Delivered { write } if write.kind == WriteKind::Answer) {
            self.entered.lock().unwrap().send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_an_observer_busy_with_the_report_of_a_write_when_drained_is_polled_then_it_does_not_return_before_the_report_and_the_end()
 {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let observer = Arc::new(Blocking {
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release_rx),
    });
    let sender = ScriptedSender::new();
    let settings = settings_with(observer, Duration::from_secs(60), Duration::from_millis(1));
    let (queue, _) = DeliveryQueue::start(sender, settings, id(RUN)).expect("a queue");
    let mut end = queue.answer(content("x"));
    tokio::task::spawn_blocking(move || entered_rx.recv().unwrap())
        .await
        .unwrap();
    assert!(queue.drained().now_or_never().is_none());
    assert!((&mut end).now_or_never().is_none());
    assert!(
        queue
            .state()
            .waiting
            .iter()
            .any(|name| name.kind == WriteKind::Answer)
    );
    release_tx.send(()).unwrap();
    let state = queue.drained().await;
    assert!(state.waiting.is_empty());
    assert_eq!(end.await, Ok(()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_many_producers_on_many_threads_when_they_hand_over_then_each_producers_order_holds_and_all_are_delivered()
 {
    const PRODUCERS: u32 = 8;
    const EACH: u32 = 150;
    let rig = Arc::new(Rig::new());
    let mut tasks = Vec::new();
    for producer in 0..PRODUCERS {
        let rig = Arc::clone(&rig);
        tasks.push(tokio::spawn(async move {
            let mut ends = Vec::new();
            for n in 0..EACH {
                ends.push(rig.hand(step(producer * 1000 + n)));
                if n % 7 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            for end in ends {
                assert_eq!(end.await, Ok(()));
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let state = rig.queue.drained().await;
    assert!(state.waiting.is_empty());
    let indexes: Vec<u32> = rig
        .sender
        .attempts_of(WriteKind::StepStarted)
        .iter()
        .map(|write| match write {
            Write::Fact(crate::backend::Fact::Step { index, .. }, _) => *index,
            _ => u32::MAX,
        })
        .collect();
    assert_eq!(indexes.len() as u32, PRODUCERS * EACH);
    for producer in 0..PRODUCERS {
        let mine: Vec<u32> = indexes
            .iter()
            .copied()
            .filter(|i| i / 1000 == producer)
            .collect();
        assert_eq!(
            mine,
            (0..EACH).map(|n| producer * 1000 + n).collect::<Vec<_>>()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_gates_that_end_on_other_threads_when_writes_flow_then_every_marker_is_sent_once_and_drained_sees_all()
 {
    let rig = Arc::new(Rig::new());
    let mut gates = Vec::new();
    let mut ends = Vec::new();
    for n in 0..60u128 {
        let (tx, end) = rig.hold(marker(100 + n, 10), None);
        gates.push(tx);
        ends.push(end);
    }
    let flow = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move {
            for n in 0..200 {
                drop(rig.hand(step(n)));
                tokio::task::yield_now().await;
            }
        })
    };
    let opener = tokio::spawn(async move {
        for (n, tx) in gates.into_iter().enumerate() {
            let result = if n % 5 == 0 {
                Gate::Dropped
            } else {
                Gate::Stored
            };
            let _ = tx.send(result);
            tokio::task::yield_now().await;
        }
    });
    flow.await.unwrap();
    opener.await.unwrap();
    let state = rig.queue.drained().await;
    assert!(state.waiting.is_empty());
    let mut delivered = 0;
    for (n, end) in ends.into_iter().enumerate() {
        let result = end.await;
        assert_eq!(result.is_ok(), n % 5 != 0);
        delivered += usize::from(result.is_ok());
    }
    assert_eq!(
        rig.sender.attempts_of(WriteKind::ReadMarker).len(),
        delivered
    );
}
