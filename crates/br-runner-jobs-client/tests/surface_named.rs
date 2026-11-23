#[path = "support/silent_backend.rs"]
mod silent_backend;

use std::marker::PhantomData;
use std::time::Duration;

use br_runner_jobs_client::{
    AgentBook, AgentDeclaration, AgentKey, Arrival, ArrivalFeed, ArrivalSettings, Backend,
    BookLookup, BookedAgent, BookedAgentRef, BoxFuture, CallMode, CallNotice, CallNoticeKind,
    CallerHandle, CancelEvent, Capacity, Cause, ChildCommand, ChildSession, Content,
    ContentAttachment, CreateChild, CreateChildInput, CredentialFailure, CredentialKind,
    DeliveryQueue, DispatchMode, EndWait, EntryKind, Error, Fact, Failed, Failure, FailureKind,
    FailureReportView, Feed, Gate, IgnoreReports, IncomingTrigger, ItemArrived, ItemKind,
    ItemWatch, JobsClient, LastFailure, ListedAgent, LogLevel, LogRecord, Mail, MailAttachment,
    MailEntry, MailHandle, ModeEvent, NewMailAttachment, NewMailInput, Operation, PayloadForm,
    PresenceStatus, QueueObserver, QueueReport, QueueSettings, QueueState, QueueWriter, RawFeed,
    Recipients, Refusal, RefusalReason, RegistrationEnd, Removal, Restart, RestoredAgent,
    ResultRef, RunnerType, SavedState, SyncEnd, TriggerDelivery, TriggerFeed, TriggerSource,
    UnknownWord, UnreadItem, Unreadable, Wake, Work, WorkCore, WorkIds, Write, WriteEnd, WriteKind,
    WriteName, WriteResult,
};
use silent_backend::Silent;
use uuid::Uuid;

macro_rules! named {
    ($($item:ty),* $(,)?) => {
        $(let _ = PhantomData::<$item>;)*
    };
}

#[test]
fn every_item_of_the_surface_for_adapter_libraries_is_public_under_its_name() {
    named!(
        JobsClient,
        TriggerFeed,
        Work,
        WorkCore,
        WorkIds,
        CallerHandle,
        MailHandle,
        Arrival,
        ArrivalFeed,
        ArrivalSettings,
        EndWait,
        Feed<u8>,
        Unreadable,
        CreateChildInput,
        dyn Backend,
        dyn TriggerDelivery,
        dyn TriggerSource,
        dyn RawFeed<CancelEvent>,
        dyn ChildSession,
        BoxFuture<'static, ()>,
        IncomingTrigger,
        Fact,
        LogRecord,
        CreateChild,
        ChildCommand,
        DeliveryQueue,
        QueueSettings,
        dyn QueueObserver,
        IgnoreReports,
        QueueReport,
        QueueState,
        LastFailure,
        WriteEnd,
        WriteResult,
        Removal,
        Write,
        WriteKind,
        WriteName,
        Gate,
        QueueWriter,
        AgentBook,
        AgentKey,
        RestoredAgent,
        BookedAgent,
        BookedAgentRef,
        RegistrationEnd,
        dyn BookLookup,
        EntryKind,
        MailEntry,
        Recipients,
        ItemWatch,
        Wake,
        Content,
        ContentAttachment,
        PayloadForm,
        CallMode,
        Restart,
        FailureKind,
        Failed,
        LogLevel,
        PresenceStatus,
        Capacity,
        DispatchMode,
        ModeEvent,
        CancelEvent,
        ItemKind,
        ItemArrived,
        CallNoticeKind,
        CallNotice,
        FailureReportView,
        SyncEnd,
        ResultRef,
        MailAttachment,
        Mail,
        UnreadItem,
        NewMailInput,
        NewMailAttachment,
        ListedAgent,
        AgentDeclaration,
        SavedState,
        Error,
        Cause,
        Failure,
        CredentialFailure,
        CredentialKind,
        Operation,
        Refusal,
        RefusalReason,
        UnknownWord,
        RunnerType,
    );
}

struct Observer;

impl QueueObserver for Observer {
    fn report(&self, _report: &QueueReport) {}
}

#[tokio::test]
async fn an_external_crate_implements_the_backend_the_feeds_and_the_observer_of_the_surface() {
    let backend = Silent {
        runner_type: RunnerType::new("runner").expect("a runner type"),
    };
    let mut triggers = backend
        .open_triggers(Duration::from_secs(1))
        .await
        .expect("a source");
    let trigger = triggers.next().await.expect("a trigger");
    assert!(trigger.raw.is_empty());
    trigger.delivery.ack().await.expect("acked");
    trigger.delivery.give_back().await.expect("given back");
    trigger.delivery.terminate().await.expect("terminated");
    assert!(
        backend
            .child_command(
                ChildCommand::Finish {
                    command_id: Uuid::nil(),
                    job_id: Uuid::nil(),
                    run_id: Uuid::nil(),
                },
                Duration::from_secs(1),
            )
            .await
            .is_ok()
    );
    assert!(
        backend
            .start(Uuid::nil(), Uuid::nil(), Duration::from_secs(1))
            .await
            .is_err()
    );
    Observer.report(&QueueReport::Delivered {
        write: Write::Fact(
            Fact::Completed {
                run_id: Uuid::nil(),
            },
            Uuid::nil(),
        )
        .name(),
    });
    let _ = QueueWriter::hand_over;
    let _ = Gate::Stored;
    let _ = Wake::BookDropped;
    let _ = EntryKind::DirectMail;
    let _ = WriteKind::Started;
}
