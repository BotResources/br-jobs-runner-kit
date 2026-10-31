use br_agentic_contract::{PortError, ThreadRef};
use br_llm_messages::{Conversation, ConversationId};

use super::turn::fault;
use crate::adapter::inner::Inner;
use crate::context::{ContextContent, ContextQuery};
use crate::error::Error;

impl Inner {
    pub(crate) async fn read(&self, thread: &ThreadRef) -> Result<Conversation, PortError> {
        self.drained(thread).await;
        let context = self
            .client
            .read_context(&ContextQuery {
                thread_id: thread.as_uuid(),
                agent: None,
            })
            .await
            .map_err(fault)?
            .ok_or_else(|| fault(Error::invalid("the thread does not exist")))?;
        let ContextContent::Conversation(conversation) = context.content else {
            return Err(fault(Error::Unreadable(
                "the context holds no conversation".to_owned(),
            )));
        };
        if conversation.id().as_uuid() == thread.as_uuid() {
            return Ok(conversation);
        }
        let id =
            ConversationId::from_uuid(thread.as_uuid()).map_err(|e| fault(Error::library(&e)))?;
        Conversation::from_parts(id, conversation.turns().to_vec())
            .map_err(|e| fault(Error::library(&e)))
    }
}
