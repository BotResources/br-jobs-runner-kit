use br_agentic_contract::ThreadRef;
use br_agentic_harness_contract::StoredAnswer;
use br_llm_messages::EntryId;

use crate::adapter::fate::Fate;
use crate::adapter::inner::{Inner, locked};
use crate::error::Error;

impl Inner {
    pub(crate) async fn stored_answer(
        &self,
        thread: &ThreadRef,
        entry: EntryId,
    ) -> Result<StoredAnswer, Error> {
        let unknown = || Error::UnknownEntry(entry.as_uuid().to_string());
        let slot = locked(&self.slots)
            .get(thread)
            .cloned()
            .ok_or_else(unknown)?;
        let id = entry.as_uuid();
        loop {
            let mut changes = slot.changed.subscribe();
            let fate = locked(&slot.queue).fates.get(&id).copied();
            match fate {
                None => return Err(unknown()),
                Some(Fate::Confirmed(_)) => return Ok(StoredAnswer::Stored),
                Some(Fate::Lost(_)) => return Ok(StoredAnswer::Dropped),
                Some(Fate::Whole | Fate::Chunked(_)) => {}
            }
            if changes.changed().await.is_err() {
                return Err(Error::Store("the adapter is stopped".to_owned()));
            }
        }
    }
}
