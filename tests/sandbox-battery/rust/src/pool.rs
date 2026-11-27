use std::time::{Duration, Instant};

use br_runner_sandbox::{LABEL_POOL, LABEL_POOL_ID, LABEL_THREAD, POOL_CLAIMED};
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

pub async fn claimed_list(env: &Env) -> Res<Vec<String>> {
    let selector = format!(
        "{LABEL_POOL}={POOL_CLAIMED},{LABEL_POOL_ID}={}",
        env.pool_id()
    );
    let list = Kube::of(env)
        .json(&["get", "pods", "-l", &selector])
        .await?;
    Ok(names(&list, false))
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
    if output.code == 0 {
        return Ok(Some(serde_json::from_slice(&output.stdout)?));
    }
    if output.stderr.contains("NotFound") {
        return Ok(None);
    }
    Err(BatteryError::Command {
        command: format!("kubectl get pod {name}"),
        code: output.code,
        detail: output.stderr.chars().take(400).collect(),
    })
}

pub fn is_cold(name: &str) -> bool {
    name.contains("-cold-")
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

fn holds_image(pod: &Value, image: &str) -> bool {
    pod["spec"]["containers"]
        .as_array()
        .is_some_and(|all| all.iter().any(|c| c["image"] == image))
}

fn namespace_of(pod: &Value) -> Option<&str> {
    pod["metadata"]["namespace"].as_str()
}

async fn managed_warm_pods(env: &Env) -> Res<Vec<Value>> {
    let selector = format!("{LABEL_POOL}=warm");
    let list = Kube::cluster_wide(env)
        .json(&["get", "pods", "-A", "-l", &selector])
        .await?;
    let prefix = format!("{}-", env.ns_prefix);
    Ok(list["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|pod| namespace_of(pod).is_some_and(|ns| ns.starts_with(&prefix)))
                .cloned()
                .collect()
        })
        .unwrap_or_default())
}

pub async fn replace_warm_with_image(env: &Env, image: &str, seconds: u64) -> Res {
    let mut expected: Vec<(String, usize)> = Vec::new();
    for pod in managed_warm_pods(env)
        .await?
        .iter()
        .filter(|pod| holds_image(pod, image))
    {
        let (Some(namespace), Some(name)) = (namespace_of(pod), pod["metadata"]["name"].as_str())
        else {
            continue;
        };
        Kube::cluster_wide(env)
            .in_namespace(namespace)
            .run(&[
                "delete",
                "pod",
                name,
                "--ignore-not-found",
                "--grace-period=0",
                "--force",
                "--wait=false",
            ])
            .await?;
        match expected.iter_mut().find(|(ns, _)| ns == namespace) {
            Some((_, count)) => *count += 1,
            None => expected.push((namespace.to_owned(), 1)),
        }
    }
    let start = Instant::now();
    loop {
        let pods = managed_warm_pods(env).await?;
        let stale = pods.iter().any(|pod| holds_image(pod, image));
        let short = expected.iter().find(|(namespace, count)| {
            pods.iter()
                .filter(|pod| namespace_of(pod) == Some(namespace.as_str()) && ready(pod))
                .count()
                < *count
        });
        if !stale && short.is_none() {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            return Err(BatteryError::NotReached {
                what: format!("the warm pods with the image {image} to be replaced"),
                seconds,
            });
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
