use std::process::ExitCode;
use std::sync::Arc;

use br_runner_sandbox::{Clock, SystemClock};
use br_sandbox_keeper::bucket::S3Bucket;
use br_sandbox_keeper::cluster::KubeCluster;
use br_sandbox_keeper::{
    KeeperConfig, KeeperError, POD_NAMESPACE_FILE, Parts, StoreCredential, config_path,
    own_namespace, run,
};
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn environment_error(name: &'static str, reason: impl ToString) -> KeeperError {
    KeeperError::Environment {
        name,
        reason: reason.to_string(),
    }
}

async fn parts(config: &KeeperConfig) -> Result<Parts, KeeperError> {
    let credential = StoreCredential::from_lookup(lookup)?;
    let bucket = S3Bucket::new(
        &config.store,
        credential.access_key_id(),
        credential.secret_access_key(),
    )
    .map_err(|error| KeeperError::StoreUnreachable(error.to_string()))?;
    let api = KubeCluster::connect(config.pod_keys())
        .await
        .map_err(|error| KeeperError::ApiUnreachable(error.to_string()))?;
    let hostname = lookup("HOSTNAME").unwrap_or_else(|| "keeper".to_owned());
    let own_namespace = own_namespace(std::fs::read_to_string(POD_NAMESPACE_FILE).ok())
        .or_else(|| config.lease.namespace.clone())
        .ok_or_else(|| environment_error("lease_namespace", "no namespace for the lease"))?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    Ok(Parts {
        api: Arc::new(api),
        bucket: Arc::new(bucket),
        clock,
        identity: format!("{hostname}-{}", Uuid::now_v7()),
        own_namespace,
    })
}

fn stop_signal() -> Result<CancellationToken, KeeperError> {
    let token = CancellationToken::new();
    let mut terminate =
        signal(SignalKind::terminate()).map_err(|error| environment_error("SIGTERM", error))?;
    let stop = token.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = terminate.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        stop.cancel();
    });
    Ok(token)
}

async fn start() -> Result<(), KeeperError> {
    let path = config_path(lookup)?;
    let document = std::fs::read_to_string(&path)
        .map_err(|error| environment_error("BR_SANDBOX_CONFIG", error))?;
    let config = KeeperConfig::from_json(&document)?;
    let shutdown = stop_signal()?;
    let parts = parts(&config).await?;
    run(config, parts, shutdown).await
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().json().init();
    match start().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the keeper stopped");
            ExitCode::FAILURE
        }
    }
}
