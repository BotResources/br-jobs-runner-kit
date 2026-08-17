use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_nats::jetstream::{AckKind, Context, Message};
use br_jobs_runner_core::{CancelSet, ClaimDecision, NextDue, PresenceSession, SlotPool};
use contract_jobs::runner::{
    FailureReport, RunCompleted, RunFailed, RunStarted, Trigger, WIRE_VERSION,
};
use contract_jobs::runner_transport::{RunnerStatusFact, status_subject};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::RunnerConfig;
use crate::context::RunContext;
use crate::error::HarnessError;
use crate::facts::RunFact;
use crate::runner::{RunOutcome, Runner};
use crate::transport::cancel::{CancelEvent, CancelFeed};
use crate::transport::presence::PresencePublisher;
use crate::transport::publish::publish_facts;
use crate::transport::triggers::TriggerFeed;
use crate::transport::{Transport, publish_acked};

pub(crate) const REASON_PAYLOAD_INVALID: &str = "payload_invalid";

pub(crate) async fn run_loop<R, S>(
    config: RunnerConfig,
    runner: R,
    shutdown: S,
) -> Result<(), HarnessError>
where
    R: Runner,
    S: Future<Output = ()> + Send,
{
    let transport = Transport::bind(&config).await?;
    let mut presence = PresenceSession::new(transport.presence_ttl, transport.presence_ttl / 3)?;
    let presence_publisher = PresencePublisher::new(transport.presence_store.clone(), &config);
    let mut slots = SlotPool::new(config.slot_count());
    let mut cancels = CancelSet::new();
    let mut cancel_feed = CancelFeed::new(transport.cancel_store.clone());
    cancel_feed.open().await?;
    let mut triggers = TriggerFeed::new(
        transport.js.clone(),
        config.runner_type.segment(),
        config.ack_wait,
    );
    triggers.connect().await?;

    let (facts_sender, facts_receiver) = mpsc::unbounded_channel();
    let publisher = tokio::spawn(publish_facts(
        transport.js.clone(),
        config.runner_type.segment().clone(),
        facts_receiver,
    ));

    let runner = Arc::new(runner);
    let mut runs: JoinSet<Uuid> = JoinSet::new();
    let mut tokens: HashMap<Uuid, CancellationToken> = HashMap::new();
    let started_subject = status_subject(config.runner_type.segment(), RunnerStatusFact::Started);
    let instance_key = config.instance_key.as_str().to_owned();

    tokio::pin!(shutdown);
    let mut drain_deadline: Option<tokio::time::Instant> = None;

    loop {
        if let Some(action) = presence.poll(Instant::now()) {
            presence_publisher.apply(action).await;
        }
        if drain_deadline.is_some() && runs.is_empty() {
            break;
        }
        let heartbeat_due = match presence.next_due() {
            NextDue::Now => tokio::time::Instant::now(),
            NextDue::At(at) => tokio::time::Instant::from_std(at),
            NextDue::Never => tokio::time::Instant::now() + Duration::from_secs(3600),
        };

        tokio::select! {
            _ = &mut shutdown, if drain_deadline.is_none() => {
                drain_deadline = Some(tokio::time::Instant::now() + config.drain_timeout);
                slots.begin_drain();
                presence.begin_drain();
            }
            _ = async { tokio::time::sleep_until(drain_deadline.expect("guarded by the branch condition")).await },
                if drain_deadline.is_some() => {
                tracing::warn!("drain timeout elapsed; aborting the in-flight runs");
                runs.abort_all();
                break;
            }
            message = triggers.next(), if drain_deadline.is_none() && slots.has_free_slot() => {
                accept_trigger(
                    message,
                    &transport.js,
                    &started_subject,
                    &instance_key,
                    &mut slots,
                    &cancels,
                    &mut tokens,
                    &mut runs,
                    &runner,
                    &facts_sender,
                )
                .await;
            }
            event = cancel_feed.next() => match event {
                CancelEvent::Reset => cancels.replace_with_replay([]),
                CancelEvent::Cancelled(run_id) => {
                    cancels.apply_put(run_id);
                    if let Some(token) = tokens.get(&run_id) {
                        token.cancel();
                    }
                }
                CancelEvent::Cleared(run_id) => cancels.apply_delete(run_id),
            },
            Some(finished) = runs.join_next() => {
                if let Ok(run_id) = finished {
                    slots.release(run_id);
                    tokens.remove(&run_id);
                }
            }
            _ = tokio::time::sleep_until(heartbeat_due) => {}
        }
    }

    if let Some(action) = presence.close() {
        presence_publisher.apply(action).await;
    }
    drop(facts_sender);
    let _ = publisher.await;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "the loop's mutable state is deliberately not a struct"
)]
async fn accept_trigger<R: Runner>(
    message: Message,
    js: &Context,
    started_subject: &str,
    instance_key: &str,
    slots: &mut SlotPool,
    cancels: &CancelSet,
    tokens: &mut HashMap<Uuid, CancellationToken>,
    runs: &mut JoinSet<Uuid>,
    runner: &Arc<R>,
    facts: &mpsc::UnboundedSender<RunFact>,
) {
    let trigger: Trigger = match serde_json::from_slice(&message.payload) {
        Ok(trigger) => trigger,
        Err(error) => {
            tracing::warn!(%error, "terminating an undecodable trigger frame");
            finish_delivery(message, AckKind::Term).await;
            return;
        }
    };
    if cancels.is_cancelled(trigger.run_id) {
        finish_delivery(message, AckKind::Ack).await;
        return;
    }
    match slots.try_claim(trigger.run_id) {
        ClaimDecision::Claimed => {}
        ClaimDecision::AlreadyInFlight => {
            finish_delivery(message, AckKind::Ack).await;
            return;
        }
        ClaimDecision::AtCapacity | ClaimDecision::Draining => {
            finish_delivery(message, AckKind::Nak(None)).await;
            return;
        }
    }

    let started = RunStarted {
        version: WIRE_VERSION,
        run_id: trigger.run_id,
        instance_key: instance_key.to_owned(),
    };
    if let Err(error) = publish_acked(js, started_subject.to_owned(), &started).await {
        tracing::warn!(%error, "RunStarted was not accepted; releasing the trigger for another instance");
        slots.release(trigger.run_id);
        finish_delivery(message, AckKind::Nak(None)).await;
        return;
    }
    finish_delivery(message, AckKind::Ack).await;

    let token = CancellationToken::new();
    if cancels.is_cancelled(trigger.run_id) {
        token.cancel();
    }
    tokens.insert(trigger.run_id, token.clone());
    let ctx = RunContext::new(
        trigger.run_id,
        trigger.job_id,
        trigger.attempt,
        facts.clone(),
        token,
    );
    let runner = Arc::clone(runner);
    let facts = facts.clone();
    runs.spawn(execute_run(runner, ctx, trigger, facts));
}

async fn execute_run<R: Runner>(
    runner: Arc<R>,
    ctx: RunContext,
    trigger: Trigger,
    facts: mpsc::UnboundedSender<RunFact>,
) -> Uuid {
    let run_id = trigger.run_id;
    let outcome = match serde_json::from_value::<R::Payload>(trigger.config.unwrap_or(Value::Null))
    {
        Ok(payload) => runner.execute(ctx, payload).await,
        Err(error) => RunOutcome::Failed {
            report: FailureReport {
                kind: None,
                reason_code: REASON_PAYLOAD_INVALID.to_owned(),
                params: Value::Null,
                diagnostic: json!(error.to_string()),
            },
            retry_after: None,
        },
    };
    let fact = match outcome {
        RunOutcome::Completed => RunFact::Completed(RunCompleted {
            version: WIRE_VERSION,
            run_id,
        }),
        RunOutcome::Failed {
            report,
            retry_after,
        } => RunFact::Failed(RunFailed {
            version: WIRE_VERSION,
            run_id,
            report,
            retry_after_seconds: retry_after
                .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
        }),
    };
    let _ = facts.send(fact);
    run_id
}

async fn finish_delivery(message: Message, kind: AckKind) {
    if let Err(error) = message.ack_with(kind).await {
        tracing::warn!(%error, "acknowledging a trigger delivery failed; it may redeliver");
    }
}
