use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::env::Env;
use crate::error::Res;
use crate::proc::{Helm, Kube, value_at};

pub struct ValueGuard {
    helm: Helm,
    snapshot: Option<Value>,
}

impl ValueGuard {
    pub fn new(env: &Env) -> Self {
        Self {
            helm: Helm::of(env),
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
        let Ok(mut child) = Command::new("helm")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(snapshot.to_string().as_bytes());
        }
        let _ = child.wait();
    }
}

pub struct ScaleGuard {
    context: String,
    namespace: String,
    deploy: String,
    original: u64,
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
        let _ = Command::new("kubectl")
            .args([
                "--context",
                &self.context,
                "-n",
                &self.namespace,
                "scale",
                "deployment",
                &self.deploy,
                &format!("--replicas={}", self.original),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub struct PodReaper {
    context: String,
    namespace: String,
    pods: Vec<String>,
}

impl PodReaper {
    pub fn new(env: &Env) -> Self {
        Self {
            context: env.context.clone(),
            namespace: env.namespace(),
            pods: Vec::new(),
        }
    }

    pub fn track(&mut self, pod: impl Into<String>) {
        self.pods.push(pod.into());
    }
}

impl Drop for PodReaper {
    fn drop(&mut self) {
        for pod in &self.pods {
            let _ = Command::new("kubectl")
                .args([
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
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

pub struct CordonGuard {
    context: String,
    node: String,
}

impl CordonGuard {
    pub async fn cordon(env: &Env) -> Res<Self> {
        let kube = Kube::cluster_wide(env);
        let node = kube
            .run(&["get", "nodes", "-o", "jsonpath={.items[0].metadata.name}"])
            .await?;
        let guard = Self {
            context: env.context.clone(),
            node: node.clone(),
        };
        kube.run(&["cordon", &node]).await?;
        Ok(guard)
    }
}

impl Drop for CordonGuard {
    fn drop(&mut self) {
        let _ = Command::new("kubectl")
            .args(["--context", &self.context, "uncordon", &self.node])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
