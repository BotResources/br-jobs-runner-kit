use async_nats::jetstream::ErrorCode;
use async_nats::jetstream::context::GetStreamErrorKind;
use contract_threads::STREAMING_STREAM;

use super::Wire;
use crate::error::Error;

impl Wire {
    pub(crate) async fn check_streaming(&self) -> Result<(), Error> {
        match self.jetstream.get_stream(STREAMING_STREAM).await {
            Ok(_) => Ok(()),
            Err(error) => Err(match error.kind() {
                GetStreamErrorKind::JetStream(inner)
                    if inner.error_code() == ErrorCode::STREAM_NOT_FOUND =>
                {
                    Error::InfraMissing(format!("the stream {STREAMING_STREAM} does not exist"))
                }
                _ => Error::unavailable(format!(
                    "the stream {STREAMING_STREAM} cannot be checked: {error}"
                )),
            }),
        }
    }
}
