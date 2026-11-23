use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::oneshot;
use uuid::Uuid;

use super::memory_port::MemoryPort;
use crate::caller::{CallerHandle, CreateChildInput};
use crate::endwait::EndStore;
use crate::error::Error;
use crate::model::{Content, ResultRef, SyncEnd};
use crate::queue::WriteResult;

#[derive(Debug, Clone, PartialEq)]
pub enum Answer<T> {
    Now(T),
    Late(T),
    Never,
    Fail(Error),
}

impl Answer<WriteResult> {
    pub fn delivered() -> Self {
        Self::Now(Ok(()))
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MemoryCall {
    Upload { job_id: Uuid, content: Content },
    Create(CreateChildInput),
    Finish { job_id: Uuid, command_id: Uuid },
    Cancel { job_id: Uuid, command_id: Uuid },
    ReadResult { result: ResultRef, keep: bool },
    EndWait { child_job_id: Uuid },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum WriteCall {
    Upload,
    Create,
    Finish,
    Cancel,
}

#[derive(Default)]
pub(super) struct Script {
    pub calls: Vec<MemoryCall>,
    pub writes: HashMap<WriteCall, VecDeque<Answer<WriteResult>>>,
    pub reads: VecDeque<Answer<Content>>,
    pub held_writes: Vec<(oneshot::Sender<WriteResult>, WriteResult)>,
    pub forever: Vec<oneshot::Sender<WriteResult>>,
    pub held_reads: Vec<oneshot::Sender<()>>,
    pub late_ends: Vec<(Uuid, SyncEnd)>,
}

pub(super) struct Shared {
    pub script: Mutex<Script>,
    pub store: Mutex<Option<Arc<EndStore>>>,
}

impl Shared {
    pub(super) fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn store(&self) -> Option<Arc<EndStore>> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub struct MemoryScript {
    shared: Arc<Shared>,
}

pub fn memory_caller() -> (CallerHandle, MemoryScript) {
    let shared = Arc::new(Shared {
        script: Mutex::new(Script::default()),
        store: Mutex::new(Some(Arc::new(EndStore::new()))),
    });
    let port = MemoryPort {
        shared: Arc::clone(&shared),
    };
    (
        CallerHandle::from_port(Arc::new(port)),
        MemoryScript { shared },
    )
}

pub fn result_ref(id: Uuid) -> ResultRef {
    ResultRef(id)
}

impl MemoryScript {
    fn push_write(&self, call: WriteCall, answer: Answer<WriteResult>) {
        self.shared
            .script()
            .writes
            .entry(call)
            .or_default()
            .push_back(answer);
    }

    pub fn upload(&self, answer: Answer<WriteResult>) {
        self.push_write(WriteCall::Upload, answer);
    }

    pub fn create(&self, answer: Answer<WriteResult>) {
        self.push_write(WriteCall::Create, answer);
    }

    pub fn finish(&self, answer: Answer<WriteResult>) {
        self.push_write(WriteCall::Finish, answer);
    }

    pub fn cancel(&self, answer: Answer<WriteResult>) {
        self.push_write(WriteCall::Cancel, answer);
    }

    pub fn read_result(&self, answer: Answer<Content>) {
        self.shared.script().reads.push_back(answer);
    }

    pub fn end(&self, child_job_id: Uuid, answer: Answer<SyncEnd>) {
        let Some(store) = self.shared.store() else {
            return;
        };
        match answer {
            Answer::Now(end) => store.record(child_job_id, Ok(end)),
            Answer::Late(end) => self.shared.script().late_ends.push((child_job_id, end)),
            Answer::Never => {}
            Answer::Fail(error) => store.record(child_job_id, Err(error)),
        }
    }

    pub fn infra_missing(&self) {
        if let Some(store) = self.shared.store() {
            store.infra_missing(Error::InfraMissing("the scripted feed is gone".to_owned()));
        }
    }

    pub fn open_again(&self) {
        if let Some(store) = self.shared.store() {
            store.reopened();
        }
    }

    pub fn drop_book(&self) {
        self.shared
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    pub fn release(&self) {
        let (writes, reads, ends) = {
            let mut script = self.shared.script();
            (
                std::mem::take(&mut script.held_writes),
                std::mem::take(&mut script.held_reads),
                std::mem::take(&mut script.late_ends),
            )
        };
        for (sender, result) in writes {
            let _ = sender.send(result);
        }
        for sender in reads {
            let _ = sender.send(());
        }
        if let Some(store) = self.shared.store() {
            for (child_job_id, end) in ends {
                store.record(child_job_id, Ok(end));
            }
        }
    }

    pub fn calls(&self) -> Vec<MemoryCall> {
        self.shared.script().calls.clone()
    }
}
