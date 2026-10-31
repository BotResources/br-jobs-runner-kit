mod content;
mod decode;
mod documents;
mod items;
mod lift;
mod send;
mod state;
#[cfg(test)]
mod tests;
mod unread;

use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) use content::{attachment, payload, read_result, upload_payload, upload_result};
pub(crate) use send::send_mail;
pub(crate) use state::{save_state, saved_state};
pub(crate) use unread::{read_mail, unread};

#[derive(Debug, Clone)]
pub(crate) struct WorkIds {
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub instance_key: String,
}

impl WorkIds {
    pub(crate) fn json(&self) -> Value {
        json!({
            "jobId": self.job_id,
            "runId": self.run_id,
            "instanceKey": self.instance_key,
        })
    }
}
