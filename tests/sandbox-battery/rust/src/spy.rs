use std::sync::{Arc, Mutex, PoisonError};

use br_agentic_contract::{PortError, PortFuture};
use br_agentic_harness_contract::{
    BoxId, Command, FilePath, OpenRequest, Opened, ProcessId, RunMode, RunOutcome, SandboxPort,
    SaveReceipt,
};
use br_runner_sandbox::Sandbox;

#[derive(Clone)]
pub struct Spy {
    inner: Sandbox,
    steps: Arc<Mutex<Vec<&'static str>>>,
    boxes: Arc<Mutex<Vec<BoxId>>>,
    kills: Arc<Mutex<Vec<bool>>>,
}

impl Spy {
    pub fn over(inner: Sandbox) -> Self {
        Self {
            inner,
            steps: Arc::default(),
            boxes: Arc::default(),
            kills: Arc::default(),
        }
    }

    pub fn sandbox(&self) -> &Sandbox {
        &self.inner
    }

    pub fn port(&self) -> Arc<dyn SandboxPort> {
        Arc::new(self.clone())
    }

    pub fn steps(&self) -> Vec<&'static str> {
        self.steps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn boxes(&self) -> Vec<BoxId> {
        self.boxes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn failed_kills(&self) -> usize {
        self.kills
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|done| !**done)
            .count()
    }

    pub fn last_box(&self) -> Option<BoxId> {
        self.boxes().pop()
    }

    fn step(&self, name: &'static str) {
        self.steps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name);
    }
}

impl SandboxPort for Spy {
    fn open<'a>(&'a self, request: &'a OpenRequest) -> PortFuture<'a, Result<Opened, PortError>> {
        Box::pin(async move {
            self.step("open");
            let opened = SandboxPort::open(&self.inner, request).await?;
            self.boxes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(opened.box_id.clone());
            Ok(opened)
        })
    }

    fn run<'a>(
        &'a self,
        sandbox: &'a BoxId,
        command: &'a Command,
        mode: RunMode,
    ) -> PortFuture<'a, Result<RunOutcome, PortError>> {
        self.step("run");
        SandboxPort::run(&self.inner, sandbox, command, mode)
    }

    fn read<'a>(
        &'a self,
        sandbox: &'a BoxId,
        path: &'a FilePath,
    ) -> PortFuture<'a, Result<Vec<u8>, PortError>> {
        self.step("read");
        SandboxPort::read(&self.inner, sandbox, path)
    }

    fn write<'a>(
        &'a self,
        sandbox: &'a BoxId,
        path: &'a FilePath,
        content: Vec<u8>,
    ) -> PortFuture<'a, Result<(), PortError>> {
        self.step("write");
        SandboxPort::write(&self.inner, sandbox, path, content)
    }

    fn list<'a>(
        &'a self,
        sandbox: &'a BoxId,
        under: Option<&'a FilePath>,
    ) -> PortFuture<'a, Result<Vec<FilePath>, PortError>> {
        self.step("list");
        SandboxPort::list(&self.inner, sandbox, under)
    }

    fn kill<'a>(
        &'a self,
        sandbox: &'a BoxId,
        process: &'a ProcessId,
    ) -> PortFuture<'a, Result<(), PortError>> {
        self.step("kill");
        Box::pin(async move {
            let result = SandboxPort::kill(&self.inner, sandbox, process).await;
            self.kills
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(result.is_ok());
            result
        })
    }

    fn save<'a>(&'a self, sandbox: &'a BoxId) -> PortFuture<'a, Result<SaveReceipt, PortError>> {
        self.step("save");
        SandboxPort::save(&self.inner, sandbox)
    }

    fn release<'a>(&'a self, sandbox: &'a BoxId) -> PortFuture<'a, Result<(), PortError>> {
        self.step("release");
        SandboxPort::release(&self.inner, sandbox)
    }
}
