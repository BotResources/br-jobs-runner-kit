use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::FutureExt;
use serde_json::json;
use tokio::sync::{Semaphore, oneshot, watch};
use uuid::Uuid;

use crate::backend::{BoxFuture, CreateChild, Fact};
use crate::error::{CredentialFailure, CredentialKind, Error, Operation};
use crate::model::{
    AgentDeclaration, CallMode, Content, Failed, FailureKind, NewMailInput, PayloadForm,
};
use crate::queue::{
    DeliveryQueue, Direct, Gate, QueueObserver, QueueReport, QueueSettings, Removal, Write,
    WriteEnd, WriteKind, WriteName, WriteResult, WriteSender, WriteSession,
};

mod builders;
mod rig;
mod scripted;

pub use builders::*;
pub use rig::*;
pub use scripted::*;
