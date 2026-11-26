use std::fmt::Debug;
use std::sync::{Mutex, OnceLock};
use std::thread::ThreadId;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

static EVENTS: Mutex<Vec<(ThreadId, String)>> = Mutex::new(Vec::new());
static INSTALLED: OnceLock<()> = OnceLock::new();

struct Everything;

#[derive(Default)]
struct Fields(Vec<String>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.0.push(format!("{}={value:?}", field.name()));
    }
}

impl Subscriber for Everything {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        EVENTS.lock().expect("the log lock").push((
            std::thread::current().id(),
            format!("{} {}", event.metadata().level(), fields.0.join(" ")),
        ));
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

pub struct Capture {
    thread: ThreadId,
}

impl Capture {
    pub fn install() -> Self {
        INSTALLED.get_or_init(|| {
            let _ = tracing::subscriber::set_global_default(Everything);
        });
        Self {
            thread: std::thread::current().id(),
        }
    }

    pub fn lines(&self) -> Vec<String> {
        EVENTS
            .lock()
            .expect("the log lock")
            .iter()
            .filter(|(thread, _)| *thread == self.thread)
            .map(|(_, line)| line.clone())
            .collect()
    }
}
