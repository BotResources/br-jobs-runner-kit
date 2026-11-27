use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::env::Env;
use crate::error::Res;
use crate::guards::{exit_detail, settle};
use crate::proc::{Helm, value_at};
use crate::restore::{Registration, listen_once, register};

const WHAT: &str = "helm-values-restore";

fn reset(helm: &Helm, snapshot: &Value) -> Vec<String> {
    let mut failures = Vec::new();
    match Command::new("helm")
        .args(helm.reset_args())
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

pub struct ValueGuard {
    helm: Helm,
    evidence: PathBuf,
    registration: Option<Registration>,
}

impl ValueGuard {
    pub fn new(env: &Env) -> Self {
        Self {
            helm: Helm::of(env),
            evidence: env.evidence.clone(),
            registration: None,
        }
    }

    pub async fn set(&mut self, sets: &[(String, Value)]) -> Res {
        if self.registration.is_none() {
            let snapshot = self.helm.supplied_values().await?;
            let helm = self.helm.clone();
            listen_once();
            self.registration = Some(register(WHAT, &self.evidence, move || {
                reset(&helm, &snapshot)
            }));
        }
        self.helm.upgrade(sets).await
    }

    pub async fn current(&self, key: &str) -> Res<Option<Value>> {
        let values = self.helm.values().await?;
        Ok(value_at(&values, key).cloned())
    }
}

impl Drop for ValueGuard {
    fn drop(&mut self) {
        if let Some(registration) = self.registration.take() {
            settle(&self.evidence, WHAT, &registration.restore());
        }
    }
}
