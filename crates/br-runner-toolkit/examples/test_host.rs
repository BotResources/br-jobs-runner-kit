use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs::test_double::{ScriptedJobs, trigger_json};
use br_runner_jobs::{CancelEvent, DispatchMode, JobsClient, ModeEvent};
use br_runner_toolkit::{HostEvent, Settings, SettingsBuilder, StartArgs, TestHost};
use serde_json::json;
use uuid::Uuid;

#[path = "shared/greeter.rs"]
#[allow(dead_code)] // each example uses a part of the greeter
mod greeter;

pub async fn events_of_one_run(
    client: JobsClient,
    settings: SettingsBuilder,
) -> Result<Vec<HostEvent>, Box<dyn std::error::Error + Send + Sync>> {
    let args = StartArgs::new(greeter::graph(None)?, greeter::starter(), false);
    let host = TestHost::new(client, settings, args)?;
    Ok(host.run().await?.events)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let jobs = ScriptedJobs::new("greeter", "instance-1");
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    let passport = Passport::human(
        Uuid::now_v7(),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::from_value(json!({}))
            .map_err(|claims| format!("invalid claims: {claims}"))?,
    );
    let trigger = trigger_json(
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        &passport,
    )?;
    jobs.push_trigger(serde_json::to_vec(&trigger)?);
    let settings = Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(1)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(1))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1));
    for event in events_of_one_run(jobs.client(), settings).await? {
        println!("{event:?}");
    }
    Ok(())
}
