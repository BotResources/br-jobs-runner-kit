#![doc = include_str!("../README.md")]

mod backoff;
mod cancel;
mod presence;
mod slots;

pub use backoff::{Backoff, BackoffConfigError};
pub use cancel::CancelSet;
pub use presence::{NextDue, PresenceAction, PresenceConfigError, PresenceSession};
pub use slots::{ClaimDecision, SlotPool};
