use uuid::Uuid;

use super::inner::QueueInner;
use crate::queue::item::{Item, ItemKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fate {
    Whole,
    Chunked(Uuid),
    Confirmed(Uuid),
    Lost(Uuid),
}

impl Fate {
    fn message(self) -> Option<Uuid> {
        match self {
            Self::Whole => None,
            Self::Chunked(message) | Self::Confirmed(message) | Self::Lost(message) => {
                Some(message)
            }
        }
    }
}

impl QueueInner {
    pub(crate) fn note_accepted(&mut self, item: &Item) {
        let fate = match item.kind {
            ItemKind::WholeMessage => Fate::Whole,
            ItemKind::Chunk => Fate::Chunked(item.message),
            ItemKind::Opening | ItemKind::Closing => return,
        };
        for entry in &item.entries {
            let kept = item.kind == ItemKind::Chunk
                && self
                    .fates
                    .get(entry)
                    .is_some_and(|known| known.message() == Some(item.message));
            if !kept {
                self.fates.insert(*entry, fate);
            }
        }
    }

    pub(crate) fn note_ended(&mut self, item: &Item, confirmed: bool) {
        let settle = |message: Uuid| {
            if confirmed {
                Fate::Confirmed(message)
            } else {
                Fate::Lost(message)
            }
        };
        match item.kind {
            ItemKind::WholeMessage => {
                for entry in &item.entries {
                    self.fates.insert(*entry, settle(item.message));
                }
            }
            ItemKind::Chunk => {
                if !confirmed {
                    for entry in &item.entries {
                        self.fates.insert(*entry, Fate::Lost(item.message));
                    }
                }
            }
            ItemKind::Closing => {
                for fate in self.fates.values_mut() {
                    if *fate == Fate::Chunked(item.message) {
                        *fate = settle(item.message);
                    }
                }
            }
            ItemKind::Opening => {}
        }
    }
}
