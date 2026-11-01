use async_nats::jetstream::Context;
use contract_jobs::runner::{TriggerV2, WIRE_VERSION_V2};
use contract_jobs::runner_transport::trigger_v2_subject;
use contract_jobs::segment::SubjectSegment;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::world::account;

pub struct Responder {
    pub requests: mpsc::UnboundedReceiver<Vec<u8>>,
    task: JoinHandle<()>,
}

impl Responder {
    pub async fn start(
        client: &async_nats::Client,
        subject: String,
        reply: Option<Vec<u8>>,
    ) -> Self {
        let mut subscription = client.subscribe(subject).await.expect("a subscription");
        client.flush().await.expect("flushed");
        let (sender, requests) = mpsc::unbounded_channel();
        let answering = client.clone();
        let task = tokio::spawn(async move {
            while let Some(message) = subscription.next().await {
                let _ = sender.send(message.payload.to_vec());
                if let (Some(reply), Some(to)) = (&reply, message.reply) {
                    let _ = answering.publish(to, reply.clone().into()).await;
                }
            }
        });
        Self { requests, task }
    }

    pub async fn request(&mut self) -> Vec<u8> {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.requests.recv())
            .await
            .expect("a request came")
            .expect("the stand-in lives")
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn service_passport() -> Value {
    json!({
        "kind": "service",
        "service_account_id": account().0,
        "claims": {},
    })
}

pub fn trigger(run_id: Uuid, job_id: Uuid) -> TriggerV2 {
    TriggerV2 {
        version: WIRE_VERSION_V2,
        run_id,
        job_id,
        attempt: 1,
        parent_job_id: job_id,
        initial_job_id: job_id,
        passport: service_passport(),
        triggered_by: None,
        config: None,
        payload: None,
        mode: None,
        caller_address: None,
        restart: None,
    }
}

pub async fn publish_trigger(js: &Context, runner_type: &str, trigger: &TriggerV2) {
    let segment = SubjectSegment::runner_type(runner_type).expect("a segment");
    js.publish(
        trigger_v2_subject(&segment),
        serde_json::to_vec(trigger).expect("json").into(),
    )
    .await
    .expect("sent")
    .await
    .expect("stored");
}
