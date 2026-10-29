use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use serde_json::{Value, json};

use crate::closing::Closing;
use crate::commands;
use crate::context::{self, ContextQuery, ThreadContext};
use crate::error::Error;
use crate::message::{OpenStream, WholeMessage};
use crate::operations::{self, FailTitleJob, SetTitle, UploadAccess, UploadRequest};
use crate::reply;
use crate::stream::StreamHandle;
use crate::stream::shared::Core;
use crate::thread::NewThread;
use crate::transport::{Answer, AnswerWait, BusCommand, BusTransport, Confirmed, GraphqlTransport};

#[derive(Clone)]
pub struct ThreadsClient {
    pub(crate) bus: Arc<dyn BusTransport>,
    graphql: Arc<dyn GraphqlTransport>,
    pub(crate) actor: ServiceAccountId,
}

pub(crate) async fn answered(
    pending: AnswerWait,
    wait: Option<Duration>,
) -> Result<Confirmed, Error> {
    let answer = match wait {
        Some(limit) => tokio::time::timeout(limit, pending)
            .await
            .map_err(|_| Error::AnswerLost("no answer before the wait".to_owned()))??,
        None => pending.await?,
    };
    match answer {
        Answer::Confirmed(confirmed) => Ok(confirmed),
        Answer::Refused(refusal) => Err(Error::Refused(refusal)),
    }
}

pub(crate) async fn sent(bus: &dyn BusTransport, command: BusCommand) -> Result<AnswerWait, Error> {
    bus.ready().await?;
    bus.send(command).await
}

impl ThreadsClient {
    pub fn new(
        actor: ServiceAccountId,
        bus: Arc<dyn BusTransport>,
        graphql: Arc<dyn GraphqlTransport>,
    ) -> Self {
        Self {
            bus,
            graphql,
            actor,
        }
    }

    async fn run(&self, command: BusCommand, wait: Option<Duration>) -> Result<Confirmed, Error> {
        let pending = sent(self.bus.as_ref(), command).await?;
        answered(pending, wait).await
    }

    pub async fn create_thread(&self, thread: &NewThread) -> Result<Confirmed, Error> {
        let command = commands::create_thread(self.actor, thread)?;
        self.run(command, thread.wait).await
    }

    pub async fn write_message(&self, message: &WholeMessage) -> Result<Confirmed, Error> {
        let command = commands::whole_message(self.actor, message)?;
        self.run(command, message.wait).await
    }

    pub async fn open_stream(&self, open: &OpenStream) -> Result<StreamHandle, Error> {
        let command = commands::open_stream(self.actor, open)?;
        self.run(command, open.wait).await?;
        Ok(StreamHandle::new(Core::new(
            Arc::clone(&self.bus),
            open.message_id.as_uuid(),
        )))
    }

    pub async fn close_message(
        &self,
        closing: &Closing,
        handle: Option<&StreamHandle>,
    ) -> Result<Confirmed, Error> {
        crate::closer::close(self, closing, handle).await
    }

    pub async fn read_context(&self, query: &ContextQuery) -> Result<Option<ThreadContext>, Error> {
        let variables = json!({
            "threadId": query.thread.as_uuid(),
            "agentId": query.agent.map(|agent| agent.as_uuid().to_string()),
        });
        let data = self
            .graphql(operations::body(operations::CONTEXT, variables)?)
            .await?;
        let found = data
            .get("threadsContext")
            .cloned()
            .ok_or_else(|| Error::Unreadable("the response holds no threadsContext".to_owned()))?;
        context::parse(found, query.agent.is_some())
    }

    pub async fn set_title(&self, title: &SetTitle) -> Result<(), Error> {
        let data = self.graphql(operations::set_title(title)?).await?;
        operations::acknowledged(&data, "threadsSetTitle")
    }

    pub async fn fail_title_job(&self, fail: &FailTitleJob) -> Result<(), Error> {
        let data = self.graphql(operations::fail_title(fail)?).await?;
        operations::acknowledged(&data, "threadsFailTitleJob")
    }

    pub async fn request_upload(&self, request: &UploadRequest) -> Result<UploadAccess, Error> {
        let data = self.graphql(operations::upload(request)?).await?;
        operations::upload_access(&data)
    }

    async fn graphql(&self, body: Vec<u8>) -> Result<Value, Error> {
        reply::data_of(self.graphql.post(body).await?)
    }
}
