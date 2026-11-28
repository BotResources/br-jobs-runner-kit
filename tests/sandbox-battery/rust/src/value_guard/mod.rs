#[cfg(test)]
mod tests;

use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::Value;

use crate::env::Env;
use crate::error::Res;
use crate::guards::{exit_detail, settle};
use crate::proc::{Helm, value_at};
use crate::restore::{Registration, Registry};

const WHAT: &str = "helm-values-restore";

pub trait HelmCalls: Send + Sync + 'static {
    fn values(&self) -> impl Future<Output = Res<Value>> + Send;
    fn supplied_values(&self) -> impl Future<Output = Res<Value>> + Send;
    fn upgrade(&self, sets: &[(String, Value)]) -> impl Future<Output = Res> + Send;
    fn reset(&self, snapshot: &Value) -> Vec<String>;
}

impl HelmCalls for Helm {
    fn values(&self) -> impl Future<Output = Res<Value>> + Send {
        Helm::values(self)
    }

    fn supplied_values(&self) -> impl Future<Output = Res<Value>> + Send {
        Helm::supplied_values(self)
    }

    fn upgrade(&self, sets: &[(String, Value)]) -> impl Future<Output = Res> + Send {
        Helm::upgrade(self, sets)
    }

    fn reset(&self, snapshot: &Value) -> Vec<String> {
        let mut failures = Vec::new();
        match Command::new("helm")
            .args(self.reset_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(error) => failures.push(error.to_string()),
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take()
                    && let Err(error) = stdin.write_all(snapshot.to_string().as_bytes())
                {
                    failures.push(error.to_string());
                }
                match child.wait_with_output() {
                    Ok(output) if output.status.success() => {}
                    Ok(output) => failures.push(exit_detail(&output)),
                    Err(error) => failures.push(error.to_string()),
                }
            }
        }
        failures
    }
}

pub struct ValueGuard<H: HelmCalls = Helm> {
    helm: Arc<H>,
    evidence: PathBuf,
    registry: Arc<Registry>,
    registration: Option<Registration>,
}

impl ValueGuard<Helm> {
    pub fn new(env: &Env) -> Self {
        Self::with(Helm::of(env), env.evidence.clone(), Registry::global())
    }
}

impl<H: HelmCalls> ValueGuard<H> {
    pub fn with(helm: H, evidence: PathBuf, registry: Arc<Registry>) -> Self {
        Self {
            helm: Arc::new(helm),
            evidence,
            registry,
            registration: None,
        }
    }

    pub async fn set(&mut self, sets: &[(String, Value)]) -> Res {
        let _work = self.registry.work()?;
        if self.registration.is_none() {
            let snapshot = self.helm.supplied_values().await?;
            self.registry.listen_once()?;
            let helm = Arc::clone(&self.helm);
            self.registration = Some(
                self.registry
                    .register(WHAT, &self.evidence, move || helm.reset(&snapshot)),
            );
        }
        self.helm.upgrade(sets).await
    }

    pub async fn current(&self, key: &str) -> Res<Option<Value>> {
        let values = self.helm.values().await?;
        Ok(value_at(&values, key).cloned())
    }
}

impl<H: HelmCalls> Drop for ValueGuard<H> {
    fn drop(&mut self) {
        if let Some(registration) = self.registration.take() {
            settle(&self.evidence, WHAT, &registration.restore());
        }
    }
}
