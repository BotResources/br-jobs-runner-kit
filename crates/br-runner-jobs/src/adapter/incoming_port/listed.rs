use br_runner_jobs_client::{SequencedItem, UnreadItem};

#[derive(Clone)]
pub(super) struct Listed {
    pub seq: i64,
    pub item: UnreadItem,
}

impl From<SequencedItem> for Listed {
    fn from(sequenced: SequencedItem) -> Self {
        Self {
            seq: sequenced.seq,
            item: sequenced.item,
        }
    }
}
