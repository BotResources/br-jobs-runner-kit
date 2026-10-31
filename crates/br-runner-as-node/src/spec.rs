use br_runner_jobs::Content;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Payload {
    Inline(Content),
    Kept(Content),
    Reference {
        service: String,
        document_id: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct JobSpec {
    pub payload: Payload,
    pub config: Option<Value>,
}

impl JobSpec {
    pub fn new(payload: Payload) -> Self {
        Self {
            payload,
            config: None,
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: Value) -> Self {
        self.config = Some(config);
        self
    }
}
