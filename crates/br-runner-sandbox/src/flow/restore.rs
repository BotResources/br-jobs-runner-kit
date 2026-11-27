use br_agentic_harness_contract::{FreshReason, TurnId};
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::cluster::{ExecIo, ExecStdin};
use crate::decide::{RestoreRecheck, recheck_after_restore, restore_failed};
use crate::env::Env;
use crate::error::Error;
use crate::exec::{ExecError, argv, close, drain, exit_code, expect_success};
use crate::formats::{FormatError, Manifest};
use crate::save::PartDecoder;
use crate::store::PartStream;

const OPERATION: &str = "restore";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreVerdict {
    Restored,
    Fresh(FreshReason),
}

async fn write_raw(stdin: &mut ExecStdin, raw: &[u8]) -> Result<(), Error> {
    stdin
        .write_all(raw)
        .await
        .map_err(|error| ExecError::io(OPERATION, error).into())
}

async fn feed(
    mut stdin: ExecStdin,
    mut parts: PartStream,
    mut decoder: PartDecoder,
) -> Result<(), Error> {
    while let Some(chunk) = parts.next().await {
        let raw = decoder.push(&chunk?)?;
        write_raw(&mut stdin, &raw).await?;
    }
    let tail = decoder.finish()?;
    write_raw(&mut stdin, &tail).await?;
    close(OPERATION, stdin).await?;
    Ok(())
}

impl Env {
    pub async fn restore(&self, pod: &str, manifest: &Manifest, parent: TurnId) -> RestoreVerdict {
        let bounded = tokio::time::timeout(
            self.config.restore_timeout(),
            self.restore_and_recheck(pod, manifest),
        )
        .await;
        match bounded {
            Ok(Ok(verdict)) => verdict,
            Ok(Err(_)) | Err(_) => RestoreVerdict::Fresh(restore_failed(parent)),
        }
    }

    async fn restore_and_recheck(
        &self,
        pod: &str,
        manifest: &Manifest,
    ) -> Result<RestoreVerdict, Error> {
        self.extract_part(pod, manifest).await?;
        let second = match self.load_manifest(manifest.key()).await {
            Ok(second) => second,
            Err(_) => return Ok(RestoreVerdict::Fresh(FreshReason::StoreUnavailable)),
        };
        let verdict = recheck_after_restore(
            manifest,
            second.as_ref(),
            self.clock.now(),
            self.config.save_ttl(),
        );
        Ok(match verdict {
            RestoreRecheck::Keep => RestoreVerdict::Restored,
            RestoreRecheck::Expired(reason) => RestoreVerdict::Fresh(reason),
        })
    }

    async fn extract_part(&self, pod: &str, manifest: &Manifest) -> Result<(), Error> {
        if manifest.part().bytes_raw == 0 {
            return Err(FormatError::RawLength {
                expected: 1,
                found: 0,
            }
            .into());
        }
        let parts =
            self.store
                .get_stream(&manifest.part().key)
                .await?
                .ok_or(FormatError::StoredSize {
                    expected: manifest.part().bytes_stored,
                    found: 0,
                })?;
        let decoder = PartDecoder::new(manifest.part())?;
        let command =
            argv::tar_restore_scoped(self.config.tools_path(), self.config.save_scope_paths());
        let request = self.agent_request(pod, command, true);
        let ExecIo {
            stdin,
            stdout,
            stderr,
            status,
        } = self.cluster.exec(request).await?;
        let stdin = stdin.ok_or(ExecError::Output {
            operation: OPERATION,
        })?;
        let (code, (), (), ()) = tokio::try_join!(
            async { Ok::<_, Error>(exit_code(OPERATION, status).await?) },
            feed(stdin, parts, decoder),
            async { Ok::<_, Error>(drain(OPERATION, stdout).await?) },
            async { Ok::<_, Error>(drain(OPERATION, stderr).await?) },
        )?;
        expect_success(OPERATION, code)?;
        Ok(())
    }
}
