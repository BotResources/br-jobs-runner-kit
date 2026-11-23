use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use uuid::Uuid;

use crate::backend::Backend;
use crate::graphql::WorkIds;
use crate::mail::MailHandle;

pub struct WorkCore {
    pub(crate) backend: Arc<dyn Backend>,
    pub(crate) job_id: Uuid,
    pub(crate) run_id: Uuid,
    pub(crate) parent_job_id: Uuid,
    pub(crate) initial_job_id: Uuid,
    pub(crate) passport_json: Value,
    pub(crate) queue_made: AtomicBool,
    pub(crate) book_made: AtomicBool,
}

impl WorkCore {
    pub fn ids(&self) -> WorkIds {
        WorkIds {
            job_id: self.job_id,
            run_id: self.run_id,
            instance_key: self.backend.instance_key().to_owned(),
        }
    }

    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    pub fn mail_handle(self: &Arc<Self>) -> MailHandle {
        MailHandle::new(Arc::clone(self))
    }

    pub(crate) fn claim_once(flag: &AtomicBool, what: &str) -> Result<(), crate::Error> {
        if flag.swap(true, Ordering::SeqCst) {
            return Err(crate::Error::invalid(format!(
                "the {what} of this Work was made already"
            )));
        }
        Ok(())
    }
}
