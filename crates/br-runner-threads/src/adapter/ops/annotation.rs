use std::sync::Arc;

use br_agentic_contract::{PortError, ThreadRef};
use br_llm_messages::{EntryId, TurnId};
use uuid::Uuid;

use super::turn::{closed_turn, fault};
use crate::adapter::inner::Inner;
use crate::annotation::Annotation;
use crate::error::Error;
use crate::stream::writes::annotation_chunk;

impl Inner {
    pub(crate) async fn add_annotation(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: TurnId,
        after: EntryId,
        annotation: Annotation,
    ) -> Result<(), PortError> {
        let chunk = annotation_chunk(after, &annotation).map_err(fault)?;
        let (message, entry) = (turn.as_uuid(), after.as_uuid());
        let slot = self.slot(*thread);
        self.ordered(&slot, move |inner, slot| async move {
            let mut mem = slot.mem.lock().await;
            let state = mem
                .turns
                .get(&message)
                .ok_or_else(|| Error::invalid("the adapter holds no open state of this turn"))?;
            if state.closed {
                return Err(closed_turn());
            }
            if !state.entries.contains(&entry) {
                return Err(Error::invalid(
                    "the entry is not one that this adapter queued into this turn",
                ));
            }
            inner
                .push_chunk(&slot, &mut mem, message, Vec::new(), chunk, Uuid::now_v7())
                .await
        })
        .await
        .map_err(fault)
    }
}
