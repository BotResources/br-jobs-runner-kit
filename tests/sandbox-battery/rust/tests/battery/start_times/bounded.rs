use std::time::{Duration, Instant};

use battery::adapter::{self, Tunables};
use battery::common::timed_open;
use battery::env::Env;
use battery::error::Res;
use battery::ids::Lane;
use battery::seeds;

const SEED_EXEC_BOUND_SECONDS: u64 = 120;
const OPEN_OUTER_BOUND_SECONDS: u64 = 300;
const ADAPTER_BOUND_TEXT: &str = "did not end in the time that the configuration gives";
const POD_GONE_SECONDS: u64 = 60;

#[derive(Debug, Default)]
pub(super) struct BoundedSeries {
    pub times: Vec<f64>,
    pub ended_ok: usize,
    pub stopped_by_adapter_bound: usize,
    pub stopped_by_outer_bound: usize,
    pub stopped_by_other_error: usize,
}

impl BoundedSeries {
    pub(super) fn stopped(&self) -> usize {
        self.stopped_by_adapter_bound + self.stopped_by_outer_bound + self.stopped_by_other_error
    }

    pub(super) fn summary(&self) -> String {
        format!(
            "{} opens ended ok, {} stopped: {} by the adapter bound, {} by the outer bound, {} by another error",
            self.ended_ok,
            self.stopped(),
            self.stopped_by_adapter_bound,
            self.stopped_by_outer_bound,
            self.stopped_by_other_error
        )
    }

    fn record(&self, env: &Env, name: &str) {
        env.record(&format!("{name}_opens_ended_ok"), self.ended_ok, "count");
        env.record(
            &format!("{name}_opens_stopped_by_adapter_bound"),
            self.stopped_by_adapter_bound,
            "count",
        );
        env.record(
            &format!("{name}_opens_stopped_by_outer_bound"),
            self.stopped_by_outer_bound,
            "count",
        );
        env.record(
            &format!("{name}_opens_stopped_by_other_error"),
            self.stopped_by_other_error,
            "count",
        );
    }
}

async fn clean_pods_of_thread(env: &Env, thread: &str) -> String {
    let pods = match battery::pool::pods_of_thread(env, thread).await {
        Ok(pods) => pods,
        Err(error) => return format!("pod name unknown, lookup by thread failed: {error}"),
    };
    if pods.is_empty() {
        return "no pod carries the thread label, the pod is gone or was never named".to_owned();
    }
    let mut states = Vec::new();
    for pod in &pods {
        let deleted = battery::pool::delete_now(env, pod).await;
        let gone = battery::pool::wait_gone(env, pod, POD_GONE_SECONDS).await;
        states.push(match (deleted, gone) {
            (Ok(()), Ok(())) => format!("{pod} was still there, deleted by name, now gone"),
            (Err(error), _) => format!("{pod} was still there, delete failed: {error}"),
            (Ok(()), Err(error)) => format!("{pod} was still there, deleted, not gone: {error}"),
        });
    }
    states.join("; ")
}

pub(super) async fn bounded_fresh_series(
    env: &Env,
    name: &str,
    set: &seeds::SeedSet,
    runs: usize,
    floor: usize,
) -> Res<BoundedSeries> {
    let mut tunables = Tunables::standard(env);
    tunables.exec_timeout = Some(SEED_EXEC_BOUND_SECONDS);
    let sandbox = tunables.connect(env).await?;
    let mut series = BoundedSeries::default();
    for run in 1..=runs {
        let lane = Lane::new()?;
        let started = Instant::now();
        let outcome = tokio::time::timeout(
            Duration::from_secs(OPEN_OUTER_BOUND_SECONDS),
            timed_open(&sandbox, lane.first()?.1, set.seed.clone()),
        )
        .await;
        let stopped_by = match outcome {
            Ok(Ok((millis, opened))) => {
                series.times.push(millis);
                series.ended_ok += 1;
                adapter::release(&sandbox, &opened.box_id).await?;
                None
            }
            Ok(Err(error)) => {
                let text = error.to_string();
                if text.contains(ADAPTER_BOUND_TEXT) {
                    series.stopped_by_adapter_bound += 1;
                    Some(("adapter bound", text))
                } else {
                    series.stopped_by_other_error += 1;
                    Some(("another error", text))
                }
            }
            Err(_) => {
                series.stopped_by_outer_bound += 1;
                Some((
                    "outer bound",
                    format!("the open did not return in {OPEN_OUTER_BOUND_SECONDS} s"),
                ))
            }
        };
        if let Some((bound, text)) = stopped_by {
            let seconds = started.elapsed().as_secs_f64();
            let pods = clean_pods_of_thread(env, &lane.thread.to_string()).await;
            env.record(
                &format!("{name}_run_{run}_open_stopped"),
                format!("after {seconds:.1} s, by the {bound}; error: {text}; pod: {pods}"),
                "event",
            );
        }
        battery::pool::wait_warm(env, floor, 180).await?;
    }
    series.record(env, name);
    Ok(series)
}
