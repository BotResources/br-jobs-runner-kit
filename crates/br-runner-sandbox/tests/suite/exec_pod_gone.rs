use br_agentic_harness_contract::{
    Command, CommandResult, FilePath, PortError, ProcessId, RunMode, RunOutcome,
};
use br_runner_sandbox::{ClusterError, Error, ExecError};

use crate::fakes::{POD, Rig, Script, rig};

const KILLED: i32 = 137;

fn is_fault(error: Error) -> bool {
    !PortError::from(error).is_refusal()
}

fn command() -> Command {
    Command::new("sleep 99").expect("a command")
}

fn path() -> FilePath {
    FilePath::new("a.txt").expect("a path")
}

async fn every_operation(rig: &Rig, code: i32) -> Vec<(&'static str, Result<(), Error>)> {
    let env = &rig.env;
    let mut results = Vec::new();
    rig.cluster.push_script(Script::exit(code));
    results.push((
        "run foreground",
        env.run(POD, &command(), RunMode::Foreground)
            .await
            .map(|_| ()),
    ));
    rig.cluster.push_script(Script::exit(code));
    results.push((
        "run background",
        env.run(POD, &command(), RunMode::Background)
            .await
            .map(|_| ()),
    ));
    rig.cluster.push_script(Script::exit(code));
    results.push(("read", env.read(POD, &path()).await.map(|_| ())));
    rig.cluster.push_script(Script::exit(code));
    results.push(("write", env.write(POD, &path(), b"x").await));
    rig.cluster.push_script(Script::exit(code));
    results.push(("list", env.list(POD, None).await.map(|_| ())));
    rig.cluster.push_script(Script::exit(code));
    results.push(("kill", env.kill(POD, &ProcessId::new("42")).await));
    results
}

fn assert_gone_fault(results: Vec<(&'static str, Result<(), Error>)>) {
    assert_eq!(results.len(), 6);
    for (operation, result) in results {
        let error = result.expect_err(operation);
        assert!(
            matches!(&error, Error::Cluster(ClusterError::Gone(name)) if name == POD),
            "{operation}: {error:?}"
        );
        assert!(is_fault(error), "{operation}");
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_status_of_137_in_a_pod_that_is_gone_is_a_port_fault_for_every_operation() {
    let rig = rig();
    rig.cluster.remove_pod(POD);
    assert_gone_fault(every_operation(&rig, KILLED).await);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_status_of_137_in_a_pod_that_is_being_deleted_is_a_port_fault_for_every_operation()
 {
    let rig = rig();
    rig.cluster.mark_deleting(POD);
    assert_gone_fault(every_operation(&rig, KILLED).await);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_status_of_137_in_an_evicted_pod_that_has_no_deletion_mark_is_a_port_fault()
{
    for phase in ["Failed", "Succeeded"] {
        let rig = rig();
        rig.cluster.set_phase(POD, phase);
        assert_gone_fault(every_operation(&rig, KILLED).await);
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_status_of_143_in_a_pod_that_is_gone_is_a_port_fault() {
    let rig = rig();
    rig.cluster.remove_pod(POD);
    rig.cluster.push_script(Script::exit(143));
    let error = rig
        .env
        .run(POD, &command(), RunMode::Foreground)
        .await
        .expect_err("a fault");
    assert!(matches!(error, Error::Cluster(ClusterError::Gone(_))));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_status_of_137_in_a_live_pod_stays_an_exit_code() {
    let rig = rig();
    rig.cluster.push_script(Script::exit(KILLED));
    let outcome = rig
        .env
        .run(POD, &command(), RunMode::Foreground)
        .await
        .expect("a result, not an error");
    assert_eq!(
        outcome,
        RunOutcome::Finished(CommandResult {
            exit_code: KILLED,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    );
    assert_eq!(rig.cluster.pod_reads(), 1);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_status_of_137_in_a_live_pod_keeps_the_other_operations_as_they_were() {
    let rig = rig();
    let results = every_operation(&rig, KILLED).await;
    for (operation, result) in results {
        match (operation, result) {
            ("run foreground", Ok(())) => {}
            ("read", Err(Error::Exec(ExecError::Unreadable { .. }))) => {}
            (
                "run background" | "write" | "list" | "kill",
                Err(Error::Exec(ExecError::Failed { code: KILLED, .. })),
            ) => {}
            (operation, other) => panic!("{operation}: {other:?}"),
        }
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_failed_pod_read_after_a_status_of_137_is_a_port_fault_never_an_exit_code() {
    let rig = rig();
    rig.cluster.fail_gets();
    let results = every_operation(&rig, KILLED).await;
    assert_eq!(results.len(), 6);
    for (operation, result) in results {
        let error = result.expect_err(operation);
        assert!(
            matches!(&error, Error::Cluster(ClusterError::Call(_))),
            "{operation}: {error:?}"
        );
        assert!(is_fault(error), "{operation}");
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_status_below_128_reads_no_pod_and_stays_an_exit_code() {
    let rig = rig();
    rig.cluster.fail_gets();
    rig.cluster.push_script(Script::exit(127));
    let outcome = rig
        .env
        .run(POD, &command(), RunMode::Foreground)
        .await
        .expect("a result, not an error");
    assert!(matches!(
        outcome,
        RunOutcome::Finished(CommandResult { exit_code: 127, .. })
    ));
    assert_eq!(rig.cluster.pod_reads(), 0);
}
