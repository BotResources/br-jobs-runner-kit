#![doc = include_str!("../README.md")]

mod annotation;
mod client;
mod closer;
mod closing;
mod commands;
mod context;
mod error;
mod ids;
mod message;
mod operations;
mod reply;
mod stream;
mod thread;
pub mod transport;

pub use annotation::{Annotation, AnnotationBody};
pub use client::ThreadsClient;
pub use closing::{Closing, ClosureKind};
pub use context::{ContextContent, ContextQuery, Member, Resource, ThreadContext};
pub use error::{Error, Refusal};
pub use message::{MessageStatus, OpenStream, PlacedAnnotation, WholeMessage};
pub use operations::{FailTitleJob, SetTitle, UploadAccess, UploadRequest};
pub use stream::{ResponseSink, SinkFactory, StreamHandle, StreamWriter};
pub use thread::{NewThread, ThreadOrigin};
