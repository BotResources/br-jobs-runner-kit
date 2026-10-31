use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::sync::oneshot;
use uuid::Uuid;

use crate::backend::{CreateChild, Fact};
use crate::error::Error;
use crate::model::{AgentDeclaration, Content, NewMailInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WriteKind {
    Started,
    Registration,
    Mail,
    ReadMarker,
    Upload,
    Create,
    Finish,
    Cancel,
    Answer,
    SavedState,
    PlanDeclared,
    StepStarted,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct WriteName {
    pub kind: WriteKind,
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Write {
    Started {
        run_id: Uuid,
        start_id: Uuid,
    },
    Registration {
        run_id: Uuid,
        agent: AgentDeclaration,
    },
    Mail {
        mail: NewMailInput,
    },
    Marker {
        run_id: Uuid,
        item_id: Uuid,
        agent_id: Uuid,
        entry_id: Uuid,
    },
    Upload {
        job_id: Uuid,
        content: Content,
    },
    Create(CreateChild),
    Finish {
        run_id: Uuid,
        job_id: Uuid,
        command_id: Uuid,
    },
    Cancel {
        run_id: Uuid,
        job_id: Uuid,
        command_id: Uuid,
    },
    Answer(Content),
    SavedState(serde_json::Value),
    Fact(Fact, Uuid),
}

impl Write {
    pub(crate) fn name(&self) -> WriteName {
        let (kind, ids) = match self {
            Self::Started { run_id, start_id } => (WriteKind::Started, vec![*run_id, *start_id]),
            Self::Registration { agent, .. } => (WriteKind::Registration, vec![agent.agent_id]),
            Self::Mail { mail } => (WriteKind::Mail, vec![mail.mail_id, mail.from_address]),
            Self::Marker {
                item_id, agent_id, ..
            } => (WriteKind::ReadMarker, vec![*item_id, *agent_id]),
            Self::Upload { job_id, .. } => (WriteKind::Upload, vec![*job_id]),
            Self::Create(create) => (WriteKind::Create, vec![create.job_id, create.command_id]),
            Self::Finish {
                job_id, command_id, ..
            } => (WriteKind::Finish, vec![*job_id, *command_id]),
            Self::Cancel {
                job_id, command_id, ..
            } => (WriteKind::Cancel, vec![*job_id, *command_id]),
            Self::Answer(_) => (WriteKind::Answer, Vec::new()),
            Self::SavedState(_) => (WriteKind::SavedState, Vec::new()),
            Self::Fact(fact, _) => match fact {
                Fact::Started { run_id, start_id } => {
                    (WriteKind::Started, vec![*run_id, *start_id])
                }
                Fact::Plan { run_id, .. } => (WriteKind::PlanDeclared, vec![*run_id]),
                Fact::Step { run_id, .. } => (WriteKind::StepStarted, vec![*run_id]),
                Fact::Completed { run_id } => (WriteKind::Completed, vec![*run_id]),
                Fact::Failed { run_id, .. } => (WriteKind::Failed, vec![*run_id]),
            },
        };
        WriteName { kind, ids }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Removal {
    Refused(Error),
    Failed(Error),
    DependsOnRemoved,
    LeftUnsent,
}

impl Removal {
    pub fn into_error(self) -> Option<Error> {
        match self {
            Self::Refused(error) | Self::Failed(error) => Some(error),
            Self::DependsOnRemoved | Self::LeftUnsent => None,
        }
    }
}

pub type WriteResult = Result<(), Removal>;

#[derive(Debug)]
pub struct WriteEnd {
    receiver: oneshot::Receiver<WriteResult>,
}

impl WriteEnd {
    pub(crate) fn pair() -> (oneshot::Sender<WriteResult>, Self) {
        let (sender, receiver) = oneshot::channel();
        (sender, Self { receiver })
    }

    pub(crate) fn ended(result: WriteResult) -> Self {
        let (sender, end) = Self::pair();
        let _ = sender.send(result);
        end
    }
}

impl Future for WriteEnd {
    type Output = WriteResult;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.receiver).poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(Removal::LeftUnsent)),
            Poll::Pending => Poll::Pending,
        }
    }
}
