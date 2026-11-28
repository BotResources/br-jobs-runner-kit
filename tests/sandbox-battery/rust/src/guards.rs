use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use br_runner_sandbox::LABEL_THREAD;

use crate::env::Env;
use crate::error::Res;
use crate::evidence::{now_utc, try_append};
use crate::keeper::{require_pods, wait_pods_gone};
use crate::proc::Kube;

pub use crate::pool_guard::PoolGuard;
pub use crate::value_guard::ValueGuard;

const KEEPER_GONE_SECONDS: u64 = 120;

pub(crate) fn report_failure(evidence: &Path, what: &str, detail: &str) {
    let line = format!(
        "{}\tfail\t{}\t{}\n",
        what,
        detail.replace(['\t', '\n', '\r'], " "),
        now_utc()
    );
    if let Err(error) = try_append(evidence, "checks.tsv", &line) {
        eprintln!("restore {what}: the evidence file cannot be written: {error}");
    }
    eprintln!("restore {what}: fail {detail}");
}

pub(crate) fn settle(evidence: &Path, what: &str, failures: &[String]) {
    for detail in failures {
        report_failure(evidence, what, detail);
    }
    if !failures.is_empty() && !std::thread::panicking() {
        panic!(
            "the restore {what} failed {} time(s): {}",
            failures.len(),
            failures.join("; ")
        );
    }
}

pub(crate) fn exit_detail(output: &Output) -> String {
    format!(
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(300)
            .collect::<String>()
    )
}

fn attempt_restore(command: &mut Command) -> Option<String> {
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(exit_detail(&output)),
        Err(error) => Some(error.to_string()),
    }
}

fn run_restore(evidence: &Path, what: &str, command: &mut Command) {
    let failures: Vec<String> = attempt_restore(command).into_iter().collect();
    settle(evidence, what, &failures);
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
        if replicas == 0 && original > 0 {
            require_pods(env).await?;
        }
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
        if replicas == 0 {
            wait_pods_gone(env, KEEPER_GONE_SECONDS).await?;
        }
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
    threads: Vec<String>,
    evidence: PathBuf,
}

impl PodReaper {
    pub fn new(env: &Env) -> Self {
        Self {
            context: env.context.clone(),
            namespace: env.namespace(),
            pods: Vec::new(),
            threads: Vec::new(),
            evidence: env.evidence.clone(),
        }
    }

    pub fn track(&mut self, pod: impl Into<String>) {
        self.pods.push(pod.into());
    }

    pub fn track_thread(&mut self, thread: impl Into<String>) {
        self.threads.push(thread.into());
    }
}

impl PodReaper {
    fn delete_args(&self, target: &[String]) -> Vec<String> {
        let mut args: Vec<String> = [
            "--context",
            &self.context,
            "-n",
            &self.namespace,
            "delete",
            "pod",
        ]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
        args.extend_from_slice(target);
        args.extend(
            ["--ignore-not-found", "--wait=false", "--grace-period=1"]
                .iter()
                .map(|arg| (*arg).to_owned()),
        );
        args
    }
}

impl Drop for PodReaper {
    fn drop(&mut self) {
        let by_name = self.pods.iter().map(|pod| (pod.clone(), vec![pod.clone()]));
        let by_thread = self.threads.iter().map(|thread| {
            (
                format!("thread {thread}"),
                vec!["-l".to_owned(), format!("{LABEL_THREAD}={thread}")],
            )
        });
        let failures: Vec<String> = by_name
            .chain(by_thread)
            .filter_map(|(what, target)| {
                attempt_restore(Command::new("kubectl").args(self.delete_args(&target)))
                    .map(|detail| format!("{what}: {detail}"))
            })
            .collect();
        settle(&self.evidence, "pod-reap", &failures);
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
