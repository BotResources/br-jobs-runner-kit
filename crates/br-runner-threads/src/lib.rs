#![doc = include_str!("../README.md")]

mod adapter;
mod annotation;
mod backend;
mod chunk_lane;
mod client;
mod closing;
mod context;
mod error;
mod from_connection;
mod ids;
mod live;
mod message;
mod operations;
mod queue;
mod response;
mod stream;
#[cfg(feature = "test-double")]
pub mod test_double;
mod thread;
mod wire;

pub use adapter::{
    EntryOutcome, ThreadAccessAdapter, ThreadAccessSettings, ThreadPortAdapter, WriteContext,
    WriteContextFn,
};
pub use annotation::{Annotation, AnnotationBody};
pub use client::ThreadClient;
pub use closing::{Closing, ClosureKind};
pub use context::{ContextContent, ContextQuery, Member, Resource, ThreadContext};
pub use error::{Cause, CredentialFailure, CredentialKind, Error, Failure, Refusal};
pub use message::{Confirmed, MessageStatus, OpenStream, PlacedAnnotation, WholeMessage};
pub use operations::{FailTitleJob, SetTitle, UploadAccess, UploadRequest};
pub use queue::{
    FailedAttempt, IgnoreReports, ItemKind, MemoryQueueStore, OldestItem, QueueObserver,
    QueueReport, QueueState, QueueStore, ReportKind, StoredItem, ThreadQueueState,
};
pub use response::{ResponseControl, ResponseSink};
pub use stream::{StreamHandle, StreamWriter};
pub use thread::{NewThread, ThreadOrigin};
