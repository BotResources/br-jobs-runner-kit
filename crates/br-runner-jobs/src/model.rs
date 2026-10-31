use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentAttachment {
    pub name: String,
    pub content_type: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub message: String,
    pub attachments: Vec<ContentAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayloadForm {
    Inline(Content),
    Kept,
    Reference { service: String, document_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallMode {
    Sync,
    Background { caller_address: Uuid },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Restart {
    pub restart_of_run_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Transient,
    Permanent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Failed {
    pub kind: FailureKind,
    pub reason_code: String,
    pub params: Value,
    pub diagnostic: Value,
    pub retry_after_seconds: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceStatus {
    Ready,
    Draining,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capacity(u32);

impl Capacity {
    pub fn new(value: u32) -> Result<Self, crate::Error> {
        if value == 0 {
            return Err(crate::Error::invalid("a capacity below 1"));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    Push,
    Claim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModeEvent {
    Mode(DispatchMode),
    NoEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CancelEvent {
    OrderMade { run_id: Uuid },
    OrderDeleted { run_id: Uuid },
    ReplayComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ItemKind {
    DirectMail,
    CcNotice,
    CallNotice,
    MailNotice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemArrived {
    pub address: Option<Uuid>,
    pub item_id: Uuid,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallNoticeKind {
    Accepted,
    QueuePosition,
    RunStarted,
    RunCompleted,
    JobFailed,
    JobCancelled,
    Refused,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FailureReportView {
    pub kind: Option<FailureKind>,
    pub reason_code: String,
    pub params: Value,
    pub diagnostic: Value,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SyncEnd {
    Completed {
        inline: Option<Content>,
        result_ref: Option<ResultRef>,
    },
    Failed {
        report: Option<FailureReportView>,
        message: String,
    },
    Cancelled {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResultRef(pub(crate) Uuid);

impl ResultRef {
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MailAttachment {
    pub attachment_id: Uuid,
    pub name: String,
    pub content_type: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Mail {
    pub mail_id: Uuid,
    pub from: Uuid,
    pub author: String,
    pub source_conversation: Option<Uuid>,
    pub subject: String,
    pub body: String,
    pub to: Vec<Uuid>,
    pub cc: Vec<Uuid>,
    pub attachments: Vec<MailAttachment>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CallNotice {
    pub item_id: Uuid,
    pub kind: CallNoticeKind,
    pub child_job_id: Option<Uuid>,
    pub position: Option<i64>,
    pub message: Option<String>,
    pub report: Option<FailureReportView>,
    pub inline: Option<Content>,
    pub result_ref: Option<ResultRef>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum UnreadItem {
    DirectMail { item_id: Uuid, mail: Mail },
    CcNotice { item_id: Uuid, mail_id: Uuid, subject: String },
    CallNotice(CallNotice),
    MailNotice { item_id: Uuid, mail_id: Option<Uuid>, text: String },
}

impl UnreadItem {
    pub fn item_id(&self) -> Uuid {
        match self {
            Self::DirectMail { item_id, .. }
            | Self::CcNotice { item_id, .. }
            | Self::MailNotice { item_id, .. } => *item_id,
            Self::CallNotice(notice) => notice.item_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NewMailInput {
    pub mail_id: Uuid,
    pub from_address: Uuid,
    pub author: String,
    pub source_conversation: Uuid,
    pub subject: String,
    pub body: String,
    pub to: Vec<Uuid>,
    pub cc: Vec<Uuid>,
    pub attachments: Vec<NewMailAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMailAttachment {
    pub attachment_id: Uuid,
    pub name: String,
    pub content_type: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListedAgent {
    pub agent_id: Uuid,
    pub run_id: Uuid,
    pub runner_type: String,
    pub running: bool,
    pub has_inbox: bool,
    pub mail_address: Option<Uuid>,
    pub restartable_now: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDeclaration {
    pub agent_id: Uuid,
    pub has_inbox: bool,
    pub can_restart: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SavedState {
    pub state: Value,
    pub agent_ids: Vec<Uuid>,
    pub mail_addresses: Vec<Uuid>,
}
