use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use uuid::Uuid;

use crate::backend::Backend;
use crate::graphql::WorkIds;

pub(crate) struct WorkCore {
    pub backend: Arc<dyn Backend>,
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub parent_job_id: Uuid,
    pub initial_job_id: Uuid,
    pub passport_json: Value,
    pub queue_made: AtomicBool,
    pub book_made: AtomicBool,
}

impl WorkCore {
    pub(crate) fn ids(&self) -> WorkIds {
        WorkIds {
            job_id: self.job_id,
            run_id: self.run_id,
            instance_key: self.backend.instance_key().to_owned(),
        }
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
