use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::env::Env;
use crate::error::Res;
use crate::evidence::{append, now_utc};
use crate::proc::{Helm, Kube, value_at};

fn restore_failed(evidence: &Path, what: &str, detail: &str) {
    let line = format!(
        "{}\tfail\t{}\t{}\n",
        what,
        detail.replace(['\t', '\n', '\r'], " "),
        now_utc()
    );
    append(evidence, "checks.tsv", &line);
    eprintln!("restore {what}: fail {detail}");
    if !std::thread::panicking() {
        panic!("the restore {what} failed: {detail}");
    }
}

fn run_restore(evidence: &Path, what: &str, command: &mut Command) {
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(output) => restore_failed(
            evidence,
            what,
            &format!(
                "exit {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(300)
                    .collect::<String>()
            ),
        ),
        Err(error) => restore_failed(evidence, what, &error.to_string()),
    }
}

pub struct ValueGuard {
    helm: Helm,
    evidence: PathBuf,
    snapshot: Option<Value>,
}

impl ValueGuard {
    pub fn new(env: &Env) -> Self {
        Self {
            helm: Helm::of(env),
            evidence: env.evidence.clone(),
            snapshot: None,
        }
    }

    pub async fn set(&mut self, sets: &[(String, Value)]) -> Res {
        if self.snapshot.is_none() {
            self.snapshot = Some(self.helm.supplied_values().await?);
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
        let Some(snapshot) = self.snapshot.take() else {
            return;
        };
        let args = self.helm.reset_args();
        let mut child = match Command::new("helm")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                restore_failed(&self.evidence, "helm-values-restore", &error.to_string());
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(snapshot.to_string().as_bytes())
        {
            restore_failed(&self.evidence, "helm-values-restore", &error.to_string());
        }
        match child.wait_with_output() {
            Ok(output) if output.status.success() => {}
            Ok(output) => restore_failed(
                &self.evidence,
                "helm-values-restore",
                &format!(
                    "exit {:?}: {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .take(300)
                        .collect::<String>()
                ),
            ),
            Err(error) => {
                restore_failed(&self.evidence, "helm-values-restore", &error.to_string());
            }
        }
    }
}

pub struct ScaleGuard {
    context: String,
    namespace: String,
    deploy: String,
    original: u64,
    evidence: PathBuf,
}

impl ScaleGuard {
    pub async fn scale_keeper(env: &Env, replicas: u64) -> Res<Self> {
        let kube = Kube::cluster_wide(env).in_namespace(&env.system_ns);
        let current = kube
            .json(&["get", "deployment", &env.keeper_deploy])
            .await?;
        let original = current["spec"]["replicas"].as_u64().unwrap_or(1);
        let guard = Self {
            context: env.context.clone(),
            namespace: env.system_ns.clone(),
            deploy: env.keeper_deploy.clone(),
            original,
            evidence: env.evidence.clone(),
        };
        kube.run(&[
            "scale",
            "deployment",
            &env.keeper_deploy,
            &format!("--replicas={replicas}"),
        ])
        .await?;
        Ok(guard)
    }
}

impl Drop for ScaleGuard {
    fn drop(&mut self) {
        run_restore(
            &self.evidence,
            "keeper-scale-restore",
            Command::new("kubectl").args([
                "--context",
                &self.context,
                "-n",
                &self.namespace,
                "scale",
                "deployment",
                &self.deploy,
                &format!("--replicas={}", self.original),
            ]),
        );
    }
}

pub struct PodReaper {
    context: String,
    namespace: String,
    pods: Vec<String>,
    evidence: PathBuf,
}

impl PodReaper {
    pub fn new(env: &Env) -> Self {
        Self {
            context: env.context.clone(),
            namespace: env.namespace(),
            pods: Vec::new(),
            evidence: env.evidence.clone(),
        }
    }

    pub fn track(&mut self, pod: impl Into<String>) {
        self.pods.push(pod.into());
    }
}

impl Drop for PodReaper {
    fn drop(&mut self) {
        for pod in &self.pods {
            run_restore(
                &self.evidence,
                "pod-reap",
                Command::new("kubectl").args([
                    "--context",
                    &self.context,
                    "-n",
                    &self.namespace,
                    "delete",
                    "pod",
                    pod,
                    "--ignore-not-found",
                    "--wait=false",
                    "--grace-period=1",
                ]),
            );
        }
    }
}

pub struct CordonGuard {
    context: String,
    node: String,
    evidence: PathBuf,
}

impl CordonGuard {
    pub async fn cordon(env: &Env) -> Res<Self> {
        let guard = Self {
            context: env.context.clone(),
            node: env.node.clone(),
            evidence: env.evidence.clone(),
        };
        Kube::cluster_wide(env).run(&["cordon", &env.node]).await?;
        Ok(guard)
    }
}

impl Drop for CordonGuard {
    fn drop(&mut self) {
        run_restore(
            &self.evidence,
            "node-uncordon",
            Command::new("kubectl").args(["--context", &self.context, "uncordon", &self.node]),
        );
    }
}
