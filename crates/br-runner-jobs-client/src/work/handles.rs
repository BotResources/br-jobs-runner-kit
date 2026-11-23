use std::sync::Arc;

use super::Work;
use crate::arrival::{ArrivalFeed, ArrivalSettings};
use crate::book::{AgentBook, RestoredAgent};
use crate::error::Error;
use crate::mail::MailHandle;
use crate::queue::DeliveryQueue;

impl Work {
    pub fn mail_handle(&self) -> MailHandle {
        MailHandle::new(Arc::clone(self.core()))
    }

    pub async fn arrival_feed(&self, settings: ArrivalSettings) -> Result<ArrivalFeed, Error> {
        ArrivalFeed::open(Arc::clone(self.core()), settings).await
    }

    pub fn agent_book(
        &self,
        queue: &DeliveryQueue,
        restored: Vec<RestoredAgent>,
        can_restart: bool,
    ) -> Result<AgentBook, Error> {
        AgentBook::make(
            Arc::clone(self.core()),
            queue.writer(),
            restored,
            can_restart,
        )
    }
}
