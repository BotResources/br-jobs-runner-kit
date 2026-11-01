use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber, subscriber::DefaultGuard};

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Default)]
pub struct Logs {
    lines: Arc<Mutex<Vec<Line>>>,
    next_span: Arc<AtomicU64>,
}

struct Text(String);

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

impl Subscriber for Logs {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(self.next_span.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut text = Text(String::new());
        event.record(&mut text);
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Line {
                level: *event.metadata().level(),
                text: text.0,
            });
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

impl Logs {
    pub fn capture() -> (Self, DefaultGuard) {
        let logs = Self::default();
        let guard = tracing::subscriber::set_default(logs.clone());
        (logs, guard)
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
