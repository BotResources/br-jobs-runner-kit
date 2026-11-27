use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::cluster::ExecOut;
use crate::error::Error;
use crate::exec::{CHUNK, ExecError};
use crate::save::PartEncoder;
use crate::store::PartUpload;

pub struct ReaderTask {
    handle: JoinHandle<Result<(), ExecError>>,
    operation: &'static str,
}

impl ReaderTask {
    pub fn spawn(operation: &'static str, stdout: ExecOut, chunks: UnboundedSender<Bytes>) -> Self {
        Self {
            handle: tokio::spawn(read_at_full_speed(operation, stdout, chunks)),
            operation,
        }
    }

    pub async fn join(&mut self) -> Result<(), ExecError> {
        match (&mut self.handle).await {
            Ok(read) => read,
            Err(error) => Err(ExecError::Io {
                operation: self.operation,
                reason: error.to_string(),
            }),
        }
    }
}

impl Drop for ReaderTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn read_at_full_speed(
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
        if chunks
            .send(Bytes::copy_from_slice(&buffer[..read]))
            .is_err()
        {
            return Ok(());
        }
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
