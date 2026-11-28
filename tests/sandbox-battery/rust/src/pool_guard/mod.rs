#[cfg(test)]
mod tests;

use std::future::Future;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::env::Env;
use crate::error::{Res, data};
use crate::guards::{exit_detail, settle};
use crate::proc::{self, SHORT, strings};
use crate::restore::{Registration, Registry};

const WHAT: &str = "pool-resource-restore";
const KIND: &str = "sandboxpools.sandbox.botresources.ai";

pub trait PoolCalls: Send + Sync + 'static {
    fn spec(&self) -> impl Future<Output = Res<Value>> + Send;
    fn patch(&self, sets: &[(String, Value)]) -> impl Future<Output = Res> + Send;
    fn reset(&self, snapshot: &Value) -> Vec<String>;
}

#[derive(Debug, Clone)]
pub struct PoolKube {
    program: String,
    context: String,
    namespace: String,
    name: String,
}

pub fn merge_body(sets: &[(String, Value)]) -> Value {
    let spec: Map<String, Value> = sets.iter().cloned().collect();
    json!({ "spec": spec })
}

pub fn reset_body(snapshot: &Value) -> Value {
    json!([{ "op": "replace", "path": "/spec", "value": snapshot }])
}

impl PoolKube {
    pub fn of(env: &Env) -> Self {
        Self::with(
            "kubectl",
            env.context.clone(),
            env.namespace(),
            env.pool_object.clone(),
        )
    }

    pub fn with(program: &str, context: String, namespace: String, name: String) -> Self {
        Self {
            program: program.to_owned(),
            context,
            namespace,
            name,
        }
    }

    fn base(&self, verb: &[&str]) -> Vec<String> {
        let mut args = strings(&["--context", &self.context, "-n", &self.namespace]);
        args.extend(strings(verb));
        args.push(KIND.to_owned());
        args.push(self.name.clone());
        args
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = self.base(&["get"]);
        args.extend(strings(&["-o", "json"]));
        args
    }

    pub fn merge_args(&self, sets: &[(String, Value)]) -> Vec<String> {
        let mut args = self.base(&["patch"]);
        args.extend(strings(&[
            "--type",
            "merge",
            "-p",
            &merge_body(sets).to_string(),
        ]));
        args
    }

    pub fn reset_args(&self, snapshot: &Value) -> Vec<String> {
        let mut args = self.base(&["patch"]);
        args.extend(strings(&[
            "--type",
            "json",
            "-p",
            &reset_body(snapshot).to_string(),
        ]));
        args
    }
}

impl PoolCalls for PoolKube {
    async fn spec(&self) -> Res<Value> {
        let args = self.get_args();
        let output = proc::run(&self.program, &args, None, SHORT).await?;
        let output = proc::succeeded(&self.program, &args, output)?;
        let pool: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| data("SandboxPool json", error.to_string()))?;
        match pool.get("spec") {
            Some(spec) if spec.is_object() => Ok(spec.clone()),
            _ => Err(data("SandboxPool", "the object has no spec")),
        }
    }

    async fn patch(&self, sets: &[(String, Value)]) -> Res {
        let args = self.merge_args(sets);
        let output = proc::run(&self.program, &args, None, SHORT).await?;
        proc::succeeded(&self.program, &args, output).map(|_| ())
    }

    fn reset(&self, snapshot: &Value) -> Vec<String> {
        match Command::new(&self.program)
            .args(self.reset_args(snapshot))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(output) if output.status.success() => Vec::new(),
            Ok(output) => vec![exit_detail(&output)],
            Err(error) => vec![error.to_string()],
        }
    }
}

pub struct PoolGuard<K: PoolCalls = PoolKube> {
    kube: Arc<K>,
    evidence: PathBuf,
    registry: Arc<Registry>,
    registration: Option<Registration>,
}

impl PoolGuard<PoolKube> {
    pub fn new(env: &Env) -> Self {
        Self::with(PoolKube::of(env), env.evidence.clone(), Registry::global())
    }
}

impl<K: PoolCalls> PoolGuard<K> {
    pub fn with(kube: K, evidence: PathBuf, registry: Arc<Registry>) -> Self {
        Self {
            kube: Arc::new(kube),
            evidence,
            registry,
            registration: None,
        }
    }

    pub async fn spec(&self) -> Res<Value> {
        self.kube.spec().await
    }

    pub async fn set(&mut self, sets: &[(String, Value)]) -> Res {
        let _work = self.registry.work()?;
        if self.registration.is_none() {
            let snapshot = self.kube.spec().await?;
            self.registry.listen_once()?;
            let kube = Arc::clone(&self.kube);
            self.registration = Some(
                self.registry
                    .register(WHAT, &self.evidence, move || kube.reset(&snapshot)),
            );
        }
        self.kube.patch(sets).await
    }
}

impl<K: PoolCalls> Drop for PoolGuard<K> {
    fn drop(&mut self) {
        if let Some(registration) = self.registration.take() {
            settle(&self.evidence, WHAT, &registration.restore());
        }
    }
}
