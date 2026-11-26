use br_agentic_harness_contract::{SaveReceipt, TurnId};
use bytes::Bytes;
use tokio::io::AsyncReadExt;

use crate::cluster::{ExecIo, ExecOut};
use crate::decide::saved_patch;
use crate::env::Env;
use crate::error::Error;
use crate::exec::{CHUNK, ExecError, argv, bounded, drain, exit_code, expect_success};
use crate::formats::{FormatError, Manifest, PartReport, Record, RecordError};
use crate::ids::{AttemptId, SaveKey};
use crate::keys::{manifest_key, part_key, record_key};
use crate::save::PartEncoder;
use crate::store::{PartUpload, PutOutcome};

const OPERATION: &str = "save";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveOutcome {
    pub receipt: SaveReceipt,
    pub pod_marked: bool,
}

fn record_error(error: &Error) -> RecordError {
    match error {
        Error::Store(_) | Error::Format(FormatError::StoredSize { .. }) => RecordError::Store,
        Error::Exec(ExecError::TimedOut { .. }) => RecordError::Timeout,
        _ => RecordError::Exec,
    }
}

async fn pump(
    mut stdout: ExecOut,
    encoder: &mut PartEncoder,
    upload: &mut dyn PartUpload,
) -> Result<(), Error> {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = stdout
            .read(&mut buffer)
            .await
            .map_err(|error| ExecError::io(OPERATION, error))?;
        if read == 0 {
            return Ok(());
        }
        let stored = encoder.push(&buffer[..read])?;
        if !stored.is_empty() {
            upload.write(Bytes::from(stored)).await?;
        }
    }
}

impl Env {
    pub async fn save(
        &self,
        pod: &str,
        key: &SaveKey,
        parent: Option<TurnId>,
    ) -> Result<SaveOutcome, Error> {
        if self.store.size(&manifest_key(key)).await?.is_some() {
            return Err(Error::AlreadySaved);
        }
        let attempt = AttemptId::mint();
        let started = Record::started(attempt, self.clock.now());
        self.store
            .put(&record_key(key), Bytes::from(started.to_vec()?))
            .await?;
        let written = bounded(
            OPERATION,
            self.config.save_timeout(),
            self.write_save(pod, key, parent, attempt),
        )
        .await;
        let manifest = match written {
            Ok(manifest) => manifest,
            Err(Error::AlreadySaved) => return Err(Error::AlreadySaved),
            Err(error) => {
                let failed = started.failed(self.clock.now(), record_error(&error));
                if let Ok(bytes) = failed.to_vec() {
                    let _ = self.store.put(&record_key(key), Bytes::from(bytes)).await;
                }
                return Err(error);
            }
        };
        if let Ok(bytes) = started.done(self.clock.now()).to_vec() {
            let _ = self.store.put(&record_key(key), Bytes::from(bytes)).await;
        }
        let receipt = manifest.receipt();
        let patch = saved_patch(
            &self.pod_keys(),
            key.turn(),
            receipt.bytes,
            manifest.made_at(),
        );
        let pod_marked = self.cluster.patch_pod(pod, &patch).await.is_ok();
        Ok(SaveOutcome {
            receipt,
            pod_marked,
        })
    }

    async fn write_save(
        &self,
        pod: &str,
        key: &SaveKey,
        parent: Option<TurnId>,
        attempt: AttemptId,
    ) -> Result<Manifest, Error> {
        let part = part_key(key, attempt);
        let report = self.upload_part(pod, &part).await?;
        let found = self.store.size(&part).await?;
        if found != Some(report.bytes_stored) {
            return Err(FormatError::StoredSize {
                expected: report.bytes_stored,
                found: found.unwrap_or_default(),
            }
            .into());
        }
        let scope = self
            .config
            .save_scope_paths()
            .iter()
            .map(|path| path.to_string())
            .collect();
        let manifest = Manifest::new(
            key.clone(),
            parent,
            attempt,
            self.clock.now(),
            scope,
            report,
        );
        let outcome = self
            .store
            .put_if_absent(&manifest_key(key), Bytes::from(manifest.to_vec()?))
            .await?;
        match outcome {
            PutOutcome::Created => Ok(manifest),
            PutOutcome::AlreadyExists => Err(Error::AlreadySaved),
        }
    }

    async fn upload_part(&self, pod: &str, part: &str) -> Result<PartReport, Error> {
        let mut upload = self.store.start_upload(part).await?;
        match self.stream_tar(pod, upload.as_mut()).await {
            Ok(report) => {
                upload.finish().await?;
                Ok(report)
            }
            Err(error) => {
                let _ = upload.abort().await;
                Err(error)
            }
        }
    }

    async fn stream_tar(
        &self,
        pod: &str,
        upload: &mut dyn PartUpload,
    ) -> Result<PartReport, Error> {
        let argv = argv::tar_create(self.config.tools_path(), self.config.save_scope_paths());
        let io = self
            .cluster
            .exec(self.agent_request(pod, argv, false))
            .await?;
        let ExecIo {
            stdout,
            stderr,
            status,
            ..
        } = io;
        let mut encoder = PartEncoder::new(self.config.save_zstd_level())?;
        let (code, (), ()) = tokio::try_join!(
            async { Ok::<_, Error>(exit_code(OPERATION, status).await?) },
            async { pump(stdout, &mut encoder, upload).await },
            async { Ok::<_, Error>(drain(OPERATION, stderr).await?) },
        )?;
        expect_success(OPERATION, code)?;
        let (tail, report) = encoder.finish()?;
        if !tail.is_empty() {
            upload.write(Bytes::from(tail)).await?;
        }
        Ok(report)
    }
}
