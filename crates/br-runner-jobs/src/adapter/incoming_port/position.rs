use br_agentic_contract::{Cursor, PortError};
use br_runner_jobs_client::Error;

use super::listed::Listed;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(super) struct Position {
    pub seq: i64,
    pub item_id: Uuid,
}

impl Position {
    pub(super) fn of(item: &Listed) -> Self {
        Self {
            seq: item.seq,
            item_id: item.item.item_id(),
        }
    }

    pub(super) fn cursor(self) -> Cursor {
        Cursor::new(format!("{}:{}", self.seq, self.item_id))
    }

    pub(super) fn parse(cursor: &Cursor) -> Result<Self, PortError> {
        cursor
            .as_str()
            .split_once(':')
            .and_then(|(seq, item_id)| {
                Some(Self {
                    seq: seq.parse().ok()?,
                    item_id: Uuid::parse_str(item_id).ok()?,
                })
            })
            .ok_or_else(|| {
                PortError::fault(Error::InvalidInput(
                    "the cursor of a listing is not one that this inbox gave".to_owned(),
                ))
            })
    }

    pub(super) fn rest_of(self, items: Vec<Listed>) -> Vec<Listed> {
        match items
            .iter()
            .position(|item| item.item.item_id() == self.item_id)
        {
            Some(position) => items.into_iter().skip(position + 1).collect(),
            None => items
                .into_iter()
                .filter(|item| item.seq > self.seq)
                .collect(),
        }
    }
}
