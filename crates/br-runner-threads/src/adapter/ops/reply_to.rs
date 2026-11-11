use std::sync::Arc;

use br_agentic_contract::{PortError, ThreadRef};
use br_llm_messages::TurnId;

use super::turn::fault;
use crate::adapter::inner::Inner;

impl Inner {
    pub(crate) async fn reply_to(
        self: &Arc<Self>,
        thread: &ThreadRef,
        message: TurnId,
    ) -> Result<(), PortError> {
        let message = message.as_uuid();
        let slot = self.slot(*thread);
        self.ordered(&slot, move |_inner, slot| async move {
            slot.mem.lock().await.reply_to = Some(message);
            Ok(())
        })
        .await
        .map_err(fault)
    }
}
