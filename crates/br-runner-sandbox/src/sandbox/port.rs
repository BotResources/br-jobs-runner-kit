use br_agentic_harness_contract::{
    BoxId, Command, FilePath, OpenRequest, Opened, PortError, PortFuture, ProcessId, RunMode,
    RunOutcome, SandboxPort, SaveReceipt,
};

use crate::error::Error;
use crate::ids::SaveKey;

use super::state::{BoxRecord, Sandbox};

fn port<T>(result: Result<T, Error>) -> Result<T, PortError> {
    result.map_err(PortError::from)
}

impl Sandbox {
    fn live_box(&self, id: &BoxId) -> Result<BoxRecord, Error> {
        self.inner
            .state()
            .boxes
            .get(id.as_str())
            .filter(|record| !record.released)
            .copied()
            .ok_or_else(|| Error::UnknownBox(id.to_string()))
    }

    pub async fn save_box(&self, id: &BoxId) -> Result<SaveReceipt, Error> {
        let record = self.live_box(id)?;
        let env = self.env();
        let key = SaveKey::of_turn(env.config.client_id(), &record.turn);
        let outcome = env.save(id.as_str(), &key, record.turn.parent).await?;
        if outcome.pod_marked
            && let Some(record) = self.inner.state().boxes.get_mut(id.as_str())
        {
            record.saved_bytes = Some(outcome.receipt.bytes);
        }
        Ok(outcome.receipt)
    }
}

impl SandboxPort for Sandbox {
    fn open<'a>(&'a self, request: &'a OpenRequest) -> PortFuture<'a, Result<Opened, PortError>> {
        Box::pin(async move { port(self.open_box(request).await) })
    }

    fn run<'a>(
        &'a self,
        sandbox: &'a BoxId,
        command: &'a Command,
        mode: RunMode,
    ) -> PortFuture<'a, Result<RunOutcome, PortError>> {
        Box::pin(async move {
            port(self.live_box(sandbox))?;
            port(self.env().run(sandbox.as_str(), command, mode).await)
        })
    }

    fn read<'a>(
        &'a self,
        sandbox: &'a BoxId,
        path: &'a FilePath,
    ) -> PortFuture<'a, Result<Vec<u8>, PortError>> {
        Box::pin(async move {
            port(self.live_box(sandbox))?;
            port(self.env().read(sandbox.as_str(), path).await)
        })
    }

    fn write<'a>(
        &'a self,
        sandbox: &'a BoxId,
        path: &'a FilePath,
        content: Vec<u8>,
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(async move {
            port(self.live_box(sandbox))?;
            port(self.env().write(sandbox.as_str(), path, &content).await)
        })
    }

    fn list<'a>(
        &'a self,
        sandbox: &'a BoxId,
        under: Option<&'a FilePath>,
    ) -> PortFuture<'a, Result<Vec<FilePath>, PortError>> {
        Box::pin(async move {
            port(self.live_box(sandbox))?;
            port(self.env().list(sandbox.as_str(), under).await)
        })
    }

    fn kill<'a>(
        &'a self,
        sandbox: &'a BoxId,
        process: &'a ProcessId,
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(async move {
            port(self.live_box(sandbox))?;
            port(self.env().kill(sandbox.as_str(), process).await)
        })
    }

    fn save<'a>(&'a self, sandbox: &'a BoxId) -> PortFuture<'a, Result<SaveReceipt, PortError>> {
        Box::pin(async move { port(self.save_box(sandbox).await) })
    }

    fn release<'a>(&'a self, sandbox: &'a BoxId) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(async move { port(self.release_box(sandbox).await) })
    }
}
