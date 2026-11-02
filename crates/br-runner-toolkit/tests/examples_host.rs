mod support;

#[path = "../examples/test_host.rs"]
#[allow(dead_code)] // the example holds code that only its own main uses
mod test_host;

use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::HostEvent;

use support::{push_mode, runner_type, trigger};

#[tokio::test(start_paused = true)]
async fn the_test_host_example_returns_the_events_of_one_run() {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let builder = br_runner_toolkit::Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(1)
        .ack_wait(std::time::Duration::from_secs(30))
        .drain_timeout(std::time::Duration::from_secs(1))
        .cancel_grace(std::time::Duration::from_secs(1))
        .claim_wait(std::time::Duration::from_secs(5))
        .outcome_wait(std::time::Duration::from_secs(10))
        .retry_wait(std::time::Duration::from_secs(1));
    let events = test_host::events_of_one_run(jobs.client(), builder)
        .await
        .expect("events");
    assert!(events.contains(&HostEvent::TriggerAcknowledged));
    assert!(events.contains(&HostEvent::PresenceDeleted));
}
