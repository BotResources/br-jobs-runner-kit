use serde_json::{Value, json};

use super::ThreadClient;
use crate::context::{self, ContextQuery, ThreadContext};
use crate::error::Error;
use crate::operations::{self, FailTitleJob, SetTitle, UploadAccess, UploadRequest};

impl ThreadClient {
    pub async fn read_context(&self, query: &ContextQuery) -> Result<Option<ThreadContext>, Error> {
        let variables = json!({
            "threadId": query.thread_id,
            "agentId": query.agent.map(|agent| agent.as_uuid().to_string()),
        });
        let data = self.backend.graphql(operations::CONTEXT, variables).await?;
        let found = data
            .get("threadsContext")
            .cloned()
            .ok_or_else(|| Error::Unreadable("the response holds no threadsContext".to_owned()))?;
        context::parse(found, query.agent.is_some())
    }

    pub async fn set_title(&self, title: &SetTitle) -> Result<(), Error> {
        let (document, variables) = operations::set_title(title);
        let data = self.run(document, variables).await?;
        operations::acknowledged(&data, "threadsSetTitle")
    }

    pub async fn fail_title_job(&self, fail: &FailTitleJob) -> Result<(), Error> {
        let (document, variables) = operations::fail_title(fail);
        let data = self.run(document, variables).await?;
        operations::acknowledged(&data, "threadsFailTitleJob")
    }

    pub async fn request_upload(&self, request: &UploadRequest) -> Result<UploadAccess, Error> {
        let (document, variables) = operations::upload(request)?;
        let data = self.run(document, variables).await?;
        operations::upload_access(&data)
    }

    async fn run(&self, document: &str, variables: Value) -> Result<Value, Error> {
        self.backend.graphql(document, variables).await
    }
}
