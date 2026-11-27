use std::collections::BTreeMap;

use crate::keys::{CLEAN_FALSE, CLEAN_TRUE, PodKeys};
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PodMarks {
    pub clean: Option<bool>,
    pub state_turn: Option<String>,
    pub saved_at: Option<Timestamp>,
    pub idle_since: Option<Timestamp>,
    pub claimed_at: Option<Timestamp>,
    pub state_bytes: Option<u64>,
    pub seed_hash: Option<String>,
}

impl PodMarks {
    pub fn read(annotations: &BTreeMap<String, String>, keys: &PodKeys) -> Self {
        let text = |key: String| annotations.get(&key).map(String::as_str);
        let time = |key: String| text(key).and_then(|value| Timestamp::parse(value).ok());
        Self {
            clean: match text(keys.clean()) {
                Some(CLEAN_TRUE) => Some(true),
                Some(CLEAN_FALSE) => Some(false),
                _ => None,
            },
            state_turn: text(keys.state_turn()).map(str::to_owned),
            saved_at: time(keys.saved_at()),
            idle_since: time(keys.idle_since()),
            claimed_at: time(keys.claimed_at()),
            state_bytes: text(keys.state_bytes()).and_then(|value| value.parse().ok()),
            seed_hash: text(keys.seed_hash()).map(str::to_owned),
        }
    }
}
