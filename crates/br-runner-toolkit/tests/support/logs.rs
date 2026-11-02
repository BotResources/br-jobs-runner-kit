use std::cell::RefCell;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Default)]
pub struct Logs {
    lines: Arc<Mutex<Vec<Line>>>,
}

thread_local! {
    static CAPTURING: RefCell<Option<Logs>> = const { RefCell::new(None) };
}

static INSTALLED: OnceLock<()> = OnceLock::new();
static SPANS: AtomicU64 = AtomicU64::new(0);

struct Router;

struct Text(String);

pub struct Capture {
    previous: Option<Logs>,
}

impl Visit for Text {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        } else {
            let _ = write!(self.0, "{}={value:?}", field.name());
        }
    }
}

impl Subscriber for Router {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(SPANS.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let _ = CAPTURING.try_with(|capturing| {
            if let Some(logs) = capturing.borrow().as_ref() {
                let mut text = Text(String::new());
                event.record(&mut text);
                logs.push(Line {
                    level: *event.metadata().level(),
                    text: text.0,
                });
            }
        });
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

impl Drop for Capture {
    fn drop(&mut self) {
        let previous = self.previous.take();
        let _ = CAPTURING.try_with(|capturing| *capturing.borrow_mut() = previous);
    }
}

impl Logs {
    pub fn capture() -> (Self, Capture) {
        INSTALLED.get_or_init(|| {
            tracing::subscriber::set_global_default(Router)
                .expect("the router of the logs is the only global subscriber of the process");
        });
        let logs = Self::default();
        let previous = CAPTURING.with(|capturing| capturing.replace(Some(logs.clone())));
        (logs, Capture { previous })
    }

    fn push(&self, line: Line) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    pub fn lines(&self) -> Vec<Line> {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn at(&self, level: Level) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|line| line.level == level)
            .map(|line| line.text)
            .collect()
    }

    pub fn containing(&self, level: Level, needle: &str) -> usize {
        self.at(level)
            .iter()
            .filter(|text| text.contains(needle))
            .count()
    }
}
