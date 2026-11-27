use battery::Session;
use battery::adapter::{self, Tunables};
use battery::common::{record_stats, timed_open};
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::ScaleGuard;
use battery::ids::Lane;
use battery::prng::Stats;
use br_agentic_harness_contract::Seed;
use br_runner_sandbox::Timestamp;

use super::support::LIMIT_MS;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cold_start_effect_of_the_init_container_and_the_image_size() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let keeper = ScaleGuard::scale_keeper(env, 0).await?;
    battery::common::scale_pool(env, 0).await?;
    battery::common::wait_until("no warm pod", 120, || async {
        Ok(battery::pool::warm_list(env, false).await?.is_empty())
    })
    .await?;
    let mut checks = Checks::new(env);
    let sandbox = Tunables::standard(env).connect(env).await?;
    let mut ready_times = Vec::new();
    let mut init_seconds = Vec::new();
    let mut image = String::new();
    for _ in 0..5 {
        let (millis, opened) = timed_open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
        ready_times.push(millis);
        if let Some(pod) = battery::pool::pod(env, opened.box_id.as_str()).await? {
            let status = &pod["status"]["initContainerStatuses"][0]["state"]["terminated"];
            if let (Some(a), Some(b)) =
                (status["startedAt"].as_str(), status["finishedAt"].as_str())
                && let (Ok(a), Ok(b)) = (Timestamp::parse(a), Timestamp::parse(b))
            {
                let span = b
                    .to_system_time()
                    .duration_since(a.to_system_time())
                    .unwrap_or_default();
                init_seconds.push(span.as_secs_f64() * 1000.0);
            }
            image = pod["spec"]["containers"][0]["image"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
        }
        adapter::release(&sandbox, &opened.box_id).await?;
        battery::pool::wait_gone_all(env).await?;
    }
    let stats = Stats::of(&ready_times, LIMIT_MS);
    record_stats(env, "cold_start_to_first_true", &stats);
    let init = Stats::of(&init_seconds, LIMIT_MS);
    env.record(
        "cold_start_init_container_median",
        format!("{:.0}", init.median),
        "ms",
    );
    let sizes = battery::pool::node_exec(env, "k3s crictl images -o json").await?;
    let parsed: serde_json::Value = serde_json::from_slice(&sizes.stdout).unwrap_or_default();
    let bytes = parsed["images"]
        .as_array()
        .and_then(|all| {
            all.iter().find(|i| {
                i["repoTags"].as_array().is_some_and(|t| {
                    t.iter().any(|x| {
                        x.as_str()
                            .is_some_and(|tag| tag == image || tag.ends_with(&format!("/{image}")))
                    })
                })
            })
        })
        .and_then(|i| i["size"].as_str().and_then(|s| s.parse::<u64>().ok()));
    env.record(
        "cold_start_image_size",
        bytes.map_or_else(|| "unknown".to_owned(), |b| b.to_string()),
        "bytes",
    );
    checks.expect(
        "cold-start-measured-with-init-container-time-and-image-size",
        !init_seconds.is_empty() && bytes.is_some(),
        format!(
            "init samples {}, image {image}, size {bytes:?}",
            init_seconds.len()
        ),
    );
    drop(keeper);
    checks.finish()
}
