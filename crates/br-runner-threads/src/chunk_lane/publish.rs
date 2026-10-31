use std::time::Duration;

use contract_threads::{Chunk, ChunkFrame};
use uuid::Uuid;

use super::Wire;
use crate::error::Error;

impl Wire {
    pub(crate) async fn publish_chunk(
        &self,
        message_id: Uuid,
        seq: u64,
        chunk: Chunk,
        wait: Duration,
    ) -> Result<(), Error> {
        if !self.connected() {
            return Err(Error::StreamBroken);
        }
        let frame = ChunkFrame::new(message_id, seq, chunk);
        let bytes = serde_json::to_vec(&frame).map_err(|e| Error::invalid(e.to_string()))?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert(async_nats::header::NATS_MESSAGE_ID, frame.msg_id());
        let sent = async {
            let ack = self
                .jetstream
                .publish_with_headers(frame.subject(), headers, bytes.into())
                .await
                .map_err(|_| Error::StreamBroken)?;
            ack.await.map_err(|_| Error::StreamBroken)?;
            Ok(())
        };
        tokio::time::timeout(wait, sent)
            .await
            .map_err(|_| Error::StreamBroken)?
    }
}
