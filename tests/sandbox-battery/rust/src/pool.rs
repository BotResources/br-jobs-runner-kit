use std::time::{Duration, Instant};

use br_runner_sandbox::{LABEL_POOL, LABEL_POOL_ID, LABEL_THREAD};
use serde_json::Value;

use crate::env::Env;
use crate::error::{BatteryError, Res};
use crate::proc::{Kube, SHORT};

fn ready(pod: &Value) -> bool {
    let running = pod["status"]["phase"] == "Running";
    let deleting = !pod["metadata"]["deletionTimestamp"].is_null();
    let condition = pod["status"]["conditions"].as_array().is_some_and(|all| {
        all.iter()
            .any(|c| c["type"] == "Ready" && c["status"] == "True")
    });
    running && condition && !deleting
}

fn names(list: &Value, only_ready: bool) -> Vec<String> {
    list["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|pod| !only_ready || ready(pod))
                .filter_map(|pod| pod["metadata"]["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn warm_list(env: &Env, only_ready: bool) -> Res<Vec<String>> {
    let selector = format!("{LABEL_POOL}=warm,{LABEL_POOL_ID}={}", env.pool_id());
    let list = Kube::of(env)
        .json(&["get", "pods", "-l", &selector])
        .await?;
    Ok(names(&list, only_ready))
}

pub async fn wait_warm(env: &Env, want: usize, seconds: u64) -> Res {
    let start = Instant::now();
    loop {
        if warm_list(env, true).await?.len() >= want {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            return Err(BatteryError::NotReached {
                what: format!("{want} ready warm pods in {}", env.namespace()),
                seconds,
            });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub async fn pod(env: &Env, name: &str) -> Res<Option<Value>> {
    let output = Kube::of(env)
        .try_run(&["get", "pod", name, "-o", "json"], None, SHORT)
        .await?;
    if output.code != 0 {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&output.stdout)?))
}

pub async fn pods_of_thread(env: &Env, thread: &str) -> Res<Vec<String>> {
    let selector = format!("{LABEL_THREAD}={thread}");
    let list = Kube::of(env)
        .json(&["get", "pods", "-l", &selector])
        .await?;
    Ok(names(&list, false))
}

pub async fn all_pods(env: &Env) -> Res<Value> {
    Kube::of(env).json(&["get", "pods"]).await
}

pub async fn replica_set(env: &Env) -> Res<Value> {
    Kube::of(env)
        .json(&["get", "replicaset", &env.replica_set()])
        .await
}

pub async fn delete_now(env: &Env, name: &str) -> Res {
    Kube::of(env)
        .run(&[
            "delete",
            "pod",
            name,
            "--ignore-not-found",
            "--grace-period=0",
            "--force",
            "--wait=false",
        ])
        .await
        .map(|_| ())
}

pub async fn wait_gone(env: &Env, name: &str, seconds: u64) -> Res {
    let start = Instant::now();
    while pod(env, name).await?.is_some() {
        if start.elapsed() > Duration::from_secs(seconds) {
            return Err(BatteryError::NotReached {
                what: format!("the pod {name} to be gone"),
                seconds,
            });
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Ok(())
}

pub async fn pod_exec(
    env: &Env,
    name: &str,
    container: &str,
    script: &str,
) -> Res<crate::proc::Output> {
    let argv = vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()];
    Kube::of(env)
        .exec(name, container, &argv, None, SHORT)
        .await
}

pub fn annotation(pod: &Value, key: &str) -> Option<String> {
    pod["metadata"]["annotations"][key]
        .as_str()
        .map(str::to_owned)
}

pub fn label(pod: &Value, key: &str) -> Option<String> {
    pod["metadata"]["labels"][key].as_str().map(str::to_owned)
}

pub async fn node_exec(env: &Env, script: &str) -> Res<crate::proc::Output> {
    let args = crate::proc::strings(&["exec", &env.node, "sh", "-c", script]);
    crate::proc::run("docker", &args, None, SHORT).await
}

pub async fn wait_gone_all(env: &Env) -> Res {
    let start = Instant::now();
    loop {
        let list = all_pods(env).await?;
        let live = list["items"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter(|p| label(p, LABEL_POOL).as_deref() == Some("claimed"))
                    .count()
            })
            .unwrap_or(0);
        if live == 0 {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(90) {
            return Err(BatteryError::NotReached {
                what: "the claimed pods to be gone".to_owned(),
                seconds: 90,
            });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
