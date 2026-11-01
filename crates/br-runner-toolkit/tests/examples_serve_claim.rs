mod support;

#[path = "../examples/serve_claim.rs"]
#[allow(dead_code)] // the main of the example is not run by the test
mod serve_claim;

use br_runner_jobs::test_double::ScriptedJobs;
use tokio_util::sync::CancellationToken;

use support::{has_terminal, kinds, trigger, until};

#[tokio::test(start_paused = true)]
async fn the_serve_claim_example_takes_a_run_in_claim_mode() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    support::claim_mode(&jobs);
    jobs.push_claim_reply(Ok(serde_json::to_vec(&serde_json::json!({
        "version": 2,
        "run": serde_json::from_slice::<serde_json::Value>(&trigger(1)).expect("trigger")
    }))
    .expect("bytes")));
    jobs.push_claim_reply(Ok(serde_json::to_vec(
        &serde_json::json!({ "version": 2, "reason": "not_in_claim_mode" }),
    )
    .expect("bytes")));
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(serve_claim::serve::serve(jobs.client(), shutdown.clone()));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("served");
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert!(jobs.deliveries().is_empty());
}
