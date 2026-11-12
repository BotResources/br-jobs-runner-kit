use std::sync::Arc;

use br_agentic_contract::{PortError, ThreadRef};
use br_llm_messages::TurnId;

use super::turn::fault;
use crate::adapter::inner::Inner;
use crate::error::Error;

impl Inner {
    pub(crate) async fn reply_to(
        self: &Arc<Self>,
        thread: &ThreadRef,
        reply: TurnId,
        parent: TurnId,
    ) -> Result<(), PortError> {
        let (reply, parent) = (reply.as_uuid(), parent.as_uuid());
        if reply == parent {
            return Err(fault(Error::invalid("a message is not its own parent")));
        }
        let slot = self.slot(*thread);
        self.ordered(&slot, move |_inner, slot| async move {
            let mut mem = slot.mem.lock().await;
            if mem.turns.contains_key(&reply) {
                return Err(Error::invalid(
                    "the adapter already queued the opening of this turn",
                ));
            }
            mem.reply_to.insert(reply, parent);
            Ok(())
        })
        .await
        .map_err(fault)
    }
}
