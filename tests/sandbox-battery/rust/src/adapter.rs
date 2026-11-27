use std::time::Duration;

use br_agentic_harness_contract::{
    BoxId, Command, CommandResult, FilePath, OpenRequest, Opened, ProcessId, RunMode, RunOutcome,
    SandboxPort, SaveReceipt, Seed, TurnRef, WebAccess,
};
use br_runner_sandbox::{Sandbox, SandboxConfig, SandboxConfigInput};
use serde_json::{Value, json};

use crate::env::Env;
use crate::error::{BatteryError, Res, data};
use crate::ids;

#[derive(Debug, Clone)]
pub struct Tunables {
    pub network_mode: String,
    pub allowlist: Vec<String>,
    pub context: String,
    pub namespace: String,
    pub pool_id: String,
    pub store_endpoint: String,
    pub store_path_style: bool,
    pub save_ttl: u64,
    pub open_timeout: u64,
    pub restore_timeout: u64,
    pub exec_timeout: Option<u64>,
    pub save_timeout: Option<u64>,
    pub idle_ttl: u64,
    pub idle_keep_min: u64,
    pub claim_attempts: u32,
    pub cold_create: bool,
    pub zstd_level: i32,
    pub scope: Vec<String>,
}

impl Tunables {
    pub fn standard(env: &Env) -> Self {
        Self {
            network_mode: env.mode.clone(),
            allowlist: Vec::new(),
            context: env.context.clone(),
            namespace: env.namespace(),
            pool_id: env.pool_id(),
            store_endpoint: env.s3.endpoint.clone(),
            store_path_style: env.s3.path_style,
            save_ttl: 3600,
            open_timeout: 30,
            restore_timeout: 120,
            exec_timeout: None,
            save_timeout: None,
            idle_ttl: 120,
            idle_keep_min: u64::MAX,
            claim_attempts: 20,
            cold_create: true,
            zstd_level: 3,
            scope: vec!["/work".to_owned(), "/home/agent".to_owned()],
        }
    }

    pub fn input(&self, env: &Env) -> Res<SandboxConfigInput> {
        let value: Value = json!({
            "namespace": self.namespace,
            "kube_context": self.context,
            "client_id": env.client,
            "pool_id": self.pool_id,
            "network_mode": self.network_mode,
            "allowlist": self.allowlist,
            "save_ttl_seconds": self.save_ttl,
            "save_scope_paths": self.scope,
            "save_zstd_level": self.zstd_level,
            "restore_timeout_seconds": self.restore_timeout,
            "open_timeout_seconds": self.open_timeout,
            "exec_timeout_seconds": self.exec_timeout,
            "save_timeout_seconds": self.save_timeout,
            "idle_ttl_seconds": self.idle_ttl,
            "idle_keep_min_state_bytes": self.idle_keep_min,
            "claim_max_attempts": self.claim_attempts,
            "cold_create": self.cold_create,
            "agent_container": env.agent_container,
            "seed_container": env.seed_container,
            "tools_path": env.agent_tools,
            "seed_tools_path": env.seed_tools,
            "seed_root": env.seed_root,
            "store_endpoint": self.store_endpoint,
            "store_bucket": env.s3.bucket,
            "store_region": env.s3.region,
            "store_path_style": self.store_path_style,
            "store_credentials": {
                "access_key_id": env.s3.access_key,
                "secret_access_key": env.s3.secret_key,
            },
        });
        serde_json::from_value(value)
            .map_err(|error| data("adapter configuration", error.to_string()))
    }

    pub fn config(&self, env: &Env) -> Res<SandboxConfig> {
        Ok(SandboxConfig::try_from(self.input(env)?)?)
    }

    pub async fn connect(&self, env: &Env) -> Res<Sandbox> {
        Ok(Sandbox::connect(self.config(env)?).await?)
    }
}

pub async fn standard(env: &Env) -> Res<Sandbox> {
    Tunables::standard(env).connect(env).await
}

pub fn request(turn: TurnRef, seed: Seed, web: Option<WebAccess>) -> Res<OpenRequest> {
    Ok(OpenRequest::new(ids::instance()?, web, turn, seed))
}

pub async fn open(sandbox: &Sandbox, turn: TurnRef, seed: Seed) -> Res<Opened> {
    let request = request(turn, seed, None)?;
    Ok(sandbox.open_box(&request).await?)
}

pub async fn sh(sandbox: &Sandbox, boxed: &BoxId, text: &str) -> Res<CommandResult> {
    let command = Command::new(text).map_err(|error| data("command", error.to_string()))?;
    match sandbox.run(boxed, &command, RunMode::Foreground).await? {
        RunOutcome::Finished(result) => Ok(result),
        RunOutcome::Started(_) => Err(data("run", "a foreground run gave a process id")),
    }
}

pub async fn sh_ok(sandbox: &Sandbox, boxed: &BoxId, text: &str) -> Res<String> {
    let result = sh(sandbox, boxed, text).await?;
    if result.exit_code != 0 {
        return Err(data(
            format!("the command `{text}`"),
            format!(
                "exit code {}, stderr {}",
                result.exit_code,
                String::from_utf8_lossy(&result.stderr)
                    .chars()
                    .take(300)
                    .collect::<String>()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

pub async fn background(sandbox: &Sandbox, boxed: &BoxId, text: &str) -> Res<ProcessId> {
    let command = Command::new(text).map_err(|error| data("command", error.to_string()))?;
    match sandbox.run(boxed, &command, RunMode::Background).await? {
        RunOutcome::Started(process) => Ok(process),
        RunOutcome::Finished(_) => Err(data("run", "a background run gave a result")),
    }
}

pub fn path(text: &str) -> Res<FilePath> {
    FilePath::new(text).map_err(|error| data(format!("the path {text}"), error.to_string()))
}

pub async fn save(sandbox: &Sandbox, boxed: &BoxId) -> Res<SaveReceipt> {
    Ok(sandbox.save_box(boxed).await?)
}

pub async fn release(sandbox: &Sandbox, boxed: &BoxId) -> Res {
    Ok(sandbox.release_box(boxed).await?)
}

pub async fn bounded<T>(
    what: &str,
    limit: Duration,
    future: impl Future<Output = Res<T>>,
) -> Res<T> {
    match tokio::time::timeout(limit, future).await {
        Ok(result) => result,
        Err(_) => Err(BatteryError::Hang {
            what: what.to_owned(),
            seconds: limit.as_secs(),
        }),
    }
}
