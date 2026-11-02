use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;

use super::*;
use crate::adapter::{NodeEntry, book_lookup};
use crate::caller::CreateChildInput;
use crate::error::Error;
use crate::model::{CallMode, PayloadForm};
use crate::queue::{QueueObserver, QueueReport};
use crate::test_double::Delivery;

pub(super) const TOKEN: &str = "tok-known-7f3a9c41-do-not-print";

pub(super) fn forbidden() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (name, n) in [
        ("job", JOB),
        ("parent job", PARENT),
        ("initial job", INITIAL),
        ("user", USER),
        ("impersonator", IMPERSONATOR),
    ] {
        found.push((format!("{name} id"), id(n).to_string()));
        found.push((format!("{name} id simple"), id(n).simple().to_string()));
    }
    found.push(("claim".to_owned(), "claim-value-ocelot".to_owned()));
    found.push(("claim email".to_owned(), "ocelot@example.test".to_owned()));
    found.push((
        "passport json".to_owned(),
        serde_json::to_string(&passport()).expect("json"),
    ));
    found.push(("token".to_owned(), TOKEN.to_owned()));
    found
}

pub(super) fn assert_clean(label: &str, text: &str) {
    for (what, needle) in forbidden() {
        assert!(!text.contains(&needle), "{label} shows the {what}: {text}");
    }
}

pub(super) fn assert_clean_except_ids(label: &str, text: &str, allowed: &[&str]) {
    for (what, needle) in forbidden() {
        if allowed.contains(&what.as_str()) {
            continue;
        }
        assert!(!text.contains(&needle), "{label} shows the {what}: {text}");
    }
}

struct Recorder(mpsc::UnboundedSender<String>);

impl QueueObserver for Recorder {
    fn report(&self, report: &QueueReport) {
        let _ = self.0.send(format!("{report:?}"));
    }
}

#[tokio::test]
async fn given_a_work_with_known_ids_and_passport_when_every_object_is_shown_then_none_shows_the_passport_the_token_or_the_three_job_ids()
 {
    let jobs = jobs();
    jobs.graphql_default(json!({}));
    let client = jobs.client();
    let mut triggers = client
        .triggers(Duration::from_secs(30))
        .await
        .expect("triggers");
    jobs.push_trigger(serde_json::to_vec(&trigger()).expect("bytes"));
    let work = triggers.next().await.map_err(|u| u.error).expect("Work");
    let (sender, mut reports) = mpsc::unbounded_channel();
    let failing = Arc::new(AtomicBool::new(true));
    let switch = Arc::clone(&failing);
    jobs.answer_facts(move || {
        if switch.load(Ordering::SeqCst) {
            Err(Error::unavailable("no listener"))
        } else {
            Ok(())
        }
    });
    let settings = QueueSettings {
        start_id: id(START),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(5),
        observer: Arc::new(Recorder(sender)),
    };
    let (queue, started) = work.delivery_queue(settings.clone()).await.expect("queue");
    let first = reports.recv().await.expect("a report");
    assert!(first.contains("Retrying"), "{first}");
    let state = format!("{:?}", queue.state());
    assert!(state.contains("last_failure"), "{state}");
    assert_clean("the queue state", &state);
    assert_clean("the queue settings", &format!("{settings:?}"));
    assert_clean("a report", &first);
    let book = work.agent_book(&queue, Vec::new(), false).expect("book");
    let caller = book.caller_handle();
    let mail = work.mail_handle();
    let create = CreateChildInput::new(
        id(300),
        id(301),
        crate::RunnerType::new("analyst").expect("a runner type"),
        CallMode::Sync,
        PayloadForm::Kept,
    );
    let created = caller.create_child(create).expect("handed over");
    let waiting = caller.end_wait(id(300));
    failing.store(false, Ordering::SeqCst);
    started.await.expect("started");
    created.await.expect("created");
    queue.drained().await;
    let mut texts = vec![first];
    while let Ok(report) = reports.try_recv() {
        texts.push(report);
    }
    for text in &texts {
        assert_clean("a report", text);
    }
    let lookup_book = Arc::new(book);
    let held = Arc::clone(&lookup_book);
    let entry = NodeEntry::new(
        "node",
        book_lookup(move |_| {
            let held = Arc::clone(&held);
            async move { Ok(held) }
        }),
    );
    let arrivals = work.arrival_feed().await.expect("arrival feed");
    let feeds = (
        format!("{:?}", client.dispatch_modes().await.expect("modes")),
        format!("{:?}", client.cancel_orders().await.expect("cancels")),
    );
    let shown = [
        ("Work debug", format!("{work:?}")),
        ("Work display", format!("{work}")),
        ("client", format!("{client:?}")),
        ("trigger feed", format!("{triggers:?}")),
        ("mode feed", feeds.0),
        ("cancel feed", feeds.1),
        ("queue", format!("{queue:?}")),
        ("started end", format!("{:?}", queue.state())),
        ("book debug", format!("{lookup_book:?}")),
        ("book display", format!("{lookup_book}")),
        ("caller debug", format!("{caller:?}")),
        ("caller display", format!("{caller}")),
        ("mail debug", format!("{mail:?}")),
        ("mail display", format!("{mail}")),
        ("end wait", format!("{waiting:?}")),
        ("arrival feed", format!("{arrivals:?}")),
        ("node entry", format!("{entry:?}")),
        ("agent port", format!("{:?}", entry.agent_port())),
    ];
    for (label, text) in shown {
        assert_clean(label, &text);
    }
    assert_eq!(jobs.deliveries(), Vec::<Delivery>::new());
}
