use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::env::Env;
use crate::error::{BatteryError, Res, data};
use crate::release_chart::ReleaseChart;

pub const SHORT: Duration = Duration::from_secs(120);
pub const LONG: Duration = Duration::from_secs(900);

#[derive(Debug, Clone)]
pub struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_owned()
    }
}

fn describe(program: &str, args: &[String]) -> String {
    let mut text = program.to_owned();
    for arg in args.iter().take(8) {
        text.push(' ');
        text.push_str(arg);
    }
    text
}

pub async fn run(
    program: &str,
    args: &[String],
    stdin: Option<Vec<u8>>,
    limit: Duration,
) -> Res<Output> {
    let command = describe(program, args);
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| BatteryError::Spawn {
            command: command.clone(),
            reason: error.to_string(),
        })?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = pipe.write_all(&bytes).await;
            let _ = pipe.shutdown().await;
        });
    }
    let waited = tokio::time::timeout(limit, child.wait_with_output())
        .await
        .map_err(|_| BatteryError::CommandTimeout {
            command: command.clone(),
            seconds: limit.as_secs(),
        })?;
    let output = waited.map_err(|error| BatteryError::Spawn {
        command,
        reason: error.to_string(),
    })?;
    Ok(Output {
        code: output.status.code().unwrap_or(-1),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

pub fn succeeded(program: &str, args: &[String], output: Output) -> Res<Output> {
    if output.code == 0 {
        Ok(output)
    } else {
        Err(BatteryError::Command {
            command: describe(program, args),
            code: output.code,
            detail: output.stderr.chars().take(400).collect(),
        })
    }
}

pub fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

pub fn sync_quiet(program: &str, args: &[&str]) {
    let _ = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[derive(Debug, Clone)]
pub struct Kube {
    pub context: String,
    pub namespace: Option<String>,
}

impl Kube {
    pub fn of(env: &Env) -> Self {
        Self {
            context: env.context.clone(),
            namespace: Some(env.namespace()),
        }
    }

    pub fn cluster_wide(env: &Env) -> Self {
        Self {
            context: env.context.clone(),
            namespace: None,
        }
    }

    pub fn in_namespace(&self, namespace: &str) -> Self {
        Self {
            context: self.context.clone(),
            namespace: Some(namespace.to_owned()),
        }
    }

    fn base(&self, args: &[&str]) -> Vec<String> {
        let mut all = vec!["--context".to_owned(), self.context.clone()];
        if let Some(namespace) = &self.namespace {
            all.push("-n".to_owned());
            all.push(namespace.clone());
        }
        all.extend(args.iter().map(|arg| (*arg).to_owned()));
        all
    }

    pub async fn try_run(
        &self,
        args: &[&str],
        stdin: Option<Vec<u8>>,
        limit: Duration,
    ) -> Res<Output> {
        run("kubectl", &self.base(args), stdin, limit).await
    }

    pub async fn run(&self, args: &[&str]) -> Res<String> {
        let all = self.base(args);
        let output = run("kubectl", &all, None, SHORT).await?;
        succeeded("kubectl", &all, output).map(|out| out.text())
    }

    pub async fn json(&self, args: &[&str]) -> Res<Value> {
        let mut all = args.to_vec();
        all.extend(["-o", "json"]);
        let text = self.run(&all).await?;
        serde_json::from_str(&text).map_err(|error| data("kubectl json", error.to_string()))
    }

    pub async fn exec(
        &self,
        pod: &str,
        container: &str,
        argv: &[String],
        stdin: Option<Vec<u8>>,
        limit: Duration,
    ) -> Res<Output> {
        let mut args = vec!["exec", pod, "-c", container];
        if stdin.is_some() {
            args.insert(1, "-i");
        }
        args.push("--");
        let mut all = self.base(&args);
        all.extend(argv.iter().cloned());
        run("kubectl", &all, stdin, limit).await
    }

    pub async fn reachable(&self) -> Res {
        let all = self.base(&["--request-timeout=10s", "get", "--raw", "/readyz"]);
        match run("kubectl", &all, None, Duration::from_secs(20)).await {
            Ok(out) if out.code == 0 => Ok(()),
            Ok(out) => Err(BatteryError::ClusterUnreachable(out.stderr)),
            Err(error) => Err(BatteryError::ClusterUnreachable(error.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Helm {
    pub context: String,
    pub release: String,
    pub namespace: String,
    pub chart: String,
    pub release_chart: ReleaseChart,
}

impl Helm {
    pub fn of(env: &Env) -> Self {
        Self {
            context: env.context.clone(),
            release: env.helm_release.clone(),
            namespace: env.helm_namespace.clone(),
            chart: env.chart_dir.display().to_string(),
            release_chart: ReleaseChart::of(env),
        }
    }

    pub async fn values(&self) -> Res<Value> {
        let args = strings(&[
            "--kube-context",
            &self.context,
            "-n",
            &self.namespace,
            "get",
            "values",
            &self.release,
            "--all",
            "-o",
            "json",
        ]);
        let output = run("helm", &args, None, SHORT).await?;
        let output = succeeded("helm", &args, output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| data("helm values", error.to_string()))
    }

    pub async fn supplied_values(&self) -> Res<Value> {
        self.release_chart.check()?;
        let args = strings(&[
            "--kube-context",
            &self.context,
            "-n",
            &self.namespace,
            "get",
            "values",
            &self.release,
            "-o",
            "json",
        ]);
        let output = run("helm", &args, None, SHORT).await?;
        let output = succeeded("helm", &args, output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| data("helm values", error.to_string()))
    }

    pub fn reset_args(&self) -> Vec<String> {
        strings(&[
            "--kube-context",
            &self.context,
            "-n",
            &self.namespace,
            "upgrade",
            &self.release,
            &self.chart,
            "--reset-values",
            "--wait=hookOnly",
            "--timeout",
            "5m",
            "-f",
            "-",
        ])
    }

    pub fn upgrade_args(&self, sets: &[(String, Value)]) -> Vec<String> {
        let mut args = strings(&[
            "--kube-context",
            &self.context,
            "-n",
            &self.namespace,
            "upgrade",
            &self.release,
            &self.chart,
            "--reuse-values",
            "--wait=hookOnly",
            "--timeout",
            "5m",
        ]);
        for (key, value) in sets {
            args.push("--set-json".to_owned());
            args.push(format!("{key}={value}"));
        }
        args
    }

    pub async fn upgrade(&self, sets: &[(String, Value)]) -> Res {
        let args = self.upgrade_args(sets);
        let output = run("helm", &args, None, LONG).await?;
        succeeded("helm", &args, output).map(|_| ())
    }
}

pub fn value_at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |node, part| node.get(part))
}
