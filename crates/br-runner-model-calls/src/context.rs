use std::collections::BTreeMap;

use uuid::Uuid;

/// What the host knows about the work that makes the calls: the ids of the
/// trigger of the jobs service, the user the work runs for, and free labels.
///
/// The library carries every value as given. It checks none of them.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCallContext {
    job_id: Uuid,
    run_id: Uuid,
    attempt: u32,
    initial_job_id: Uuid,
    user_id: Option<Uuid>,
    labels: BTreeMap<String, String>,
}

impl ModelCallContext {
    /// The context of the work of one trigger. Without [`Self::with_user`], the
    /// work runs for no user.
    pub fn new(job_id: Uuid, run_id: Uuid, attempt: u32, initial_job_id: Uuid) -> Self {
        Self {
            job_id,
            run_id,
            attempt,
            initial_job_id,
            user_id: None,
            labels: BTreeMap::new(),
        }
    }

    /// Sets the user the work runs for, replacing an earlier one.
    #[must_use]
    pub fn with_user(mut self, user_id: Uuid) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Adds a label. A label with the key of an earlier one replaces it.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn job_id(&self) -> Uuid {
        self.job_id
    }

    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn initial_job_id(&self) -> Uuid {
        self.initial_job_id
    }

    pub fn user_id(&self) -> Option<Uuid> {
        self.user_id
    }

    /// The labels, ordered by key.
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
}
