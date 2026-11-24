mod support;

use std::sync::Arc;

use br_runner_jobs_client::{BookLookup, BookedAgent, JobsClient, QueueObserver, Work};
use br_runner_toolkit::{InstanceEnded, InstanceLookup, RunStartInput, RunnerBuilder, Settings};

const MANIFEST: &str = include_str!("../Cargo.toml");

fn section(name: &str) -> &'static str {
    let header = format!("[{name}]");
    let start = MANIFEST.find(&header).expect("the section") + header.len();
    let rest = &MANIFEST[start..];
    rest.split("\n[").next().expect("the section body")
}

#[test]
fn given_the_manifest_when_the_normal_dependencies_are_read_then_the_jobs_client_is_there_and_the_adapter_library_is_not()
 {
    let normal = section("dependencies");
    assert!(normal.contains("br-runner-jobs-client"));
    assert!(!normal.contains("br-runner-jobs ="));
    assert!(!normal.contains("package = \"br-runner-jobs\""));
    assert!(normal.contains("br-agentic-contract/v0.3.0"));
}

#[test]
fn given_the_manifest_when_the_dev_dependencies_are_read_then_the_adapter_library_is_there_with_its_own_tag()
 {
    let dev = section("dev-dependencies");
    assert!(dev.contains("package = \"br-runner-jobs\""));
    assert!(dev.contains("br-runner-jobs/v0.4.0"));
    assert!(dev.contains("features = [\"test-double\"]"));
}

fn work_of<F: br_graph_contract::Fields>(input: &RunStartInput<F>) -> &Arc<Work> {
    &input.work
}

#[test]
fn given_the_public_builders_when_they_are_made_then_they_take_the_types_of_the_jobs_client() {
    let _: fn(JobsClient, Settings) -> RunnerBuilder = RunnerBuilder::new;
    let _: fn(RunnerBuilder, Arc<dyn QueueObserver>) -> RunnerBuilder =
        RunnerBuilder::with_queue_observer;
    let _: fn(&InstanceLookup) -> Arc<dyn BookLookup> = InstanceLookup::book_lookup;
    let _: fn(&InstanceEnded) -> &Vec<BookedAgent> = |ended| &ended.agents;
    let _ = work_of::<support::Flow>;
}
