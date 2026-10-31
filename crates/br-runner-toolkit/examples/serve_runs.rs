use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};
use br_runner_jobs::JobsClient;
use br_runner_toolkit::{DrainReport, RunnerBuilder, Settings, StartArgs};
use tokio_util::sync::CancellationToken;

#[path = "shared/greeter.rs"]
#[allow(dead_code)]
mod greeter;

pub fn settings() -> Result<Settings, br_runner_toolkit::Error> {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(4)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(5))
        .claim_wait(Duration::from_secs(10))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
        .build()
}

pub async fn serve(
    client: JobsClient,
    shutdown: CancellationToken,
) -> Result<DrainReport, Box<dyn std::error::Error + Send + Sync>> {
    let args =
        StartArgs::new(greeter::graph(None)?, greeter::starter(), false).with_shutdown(shutdown);
    Ok(RunnerBuilder::new(client, settings()?).start(args).await?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    let [
        _,
        endpoint,
        nats_url,
        token,
        account,
        runner_type,
        instance_key,
    ] = args.as_slice()
    else {
        return Err(
            "usage: serve_runs <endpoint> <nats-url> <token> <account-uuid> <runner-type> <instance-key>"
                .into(),
        );
    };
    let connection = Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token.as_str())),
        async_nats::connect(nats_url).await?,
        ServiceAccountId(account.parse()?),
        Duration::from_secs(30),
    )?;
    let client = JobsClient::new(
        runner_type,
        instance_key,
        Duration::from_secs(10),
        connection,
    )
    .await?;
    let shutdown = CancellationToken::new();
    let stopper = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stopper.cancel();
    });
    let report = serve(client, shutdown).await?;
    println!("{} runs left in flight", report.runs.len());
    Ok(())
}
