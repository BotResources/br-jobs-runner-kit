use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::cluster::ExecOut;
use crate::error::Error;
use crate::exec::{CHUNK, ExecError};
use crate::save::PartEncoder;
use crate::store::PartUpload;

pub async fn read_at_full_speed(
    operation: &'static str,
    mut stdout: ExecOut,
    chunks: UnboundedSender<Bytes>,
) -> Result<(), ExecError> {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = stdout
            .read(&mut buffer)
            .await
            .map_err(|error| ExecError::io(operation, error))?;
        if read == 0 {
            return Ok(());
        }
        chunks
            .send(Bytes::copy_from_slice(&buffer[..read]))
            .map_err(|_| ExecError::Io {
                operation,
                reason: "the consumer of the output stopped".to_owned(),
            })?;
    }
}

pub async fn encode_and_upload(
    mut chunks: UnboundedReceiver<Bytes>,
    encoder: &mut PartEncoder,
    upload: &mut dyn PartUpload,
) -> Result<(), Error> {
    while let Some(chunk) = chunks.recv().await {
        let stored = encoder.push(&chunk)?;
        if !stored.is_empty() {
            upload.write(Bytes::from(stored)).await?;
        }
    }
    Ok(())
}
