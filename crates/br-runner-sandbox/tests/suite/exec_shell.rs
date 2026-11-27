use br_agentic_harness_contract::{
    Command, CommandResult, PortError, ProcessId, RunMode, RunOutcome,
};
use br_runner_sandbox::{Error, ExecError, argv};
use serde_json::json;

use crate::fakes::{POD, Script, rig, rig_with};

fn command(text: &str) -> Command {
    Command::new(text).expect("a command")
}

fn is_fault(error: Error) -> bool {
    !PortError::from(error).is_refusal()
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_foreground_command_gives_its_true_exit_code_and_two_separate_streams() {
    let rig = rig();
    rig.cluster.push_script(Script {
        stdout: b"out".to_vec(),
        stderr: b"err".to_vec(),
        code: Some(3),
        ..Script::default()
    });
    let outcome = rig
        .env
        .run(POD, &command("make test"), RunMode::Foreground)
        .await
        .expect("a result, not an error");
    assert_eq!(
        outcome,
        RunOutcome::Finished(CommandResult {
            exit_code: 3,
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
        })
    );
    let execs = rig.cluster.execs();
    assert_eq!(execs.len(), 1);
    assert_eq!(execs[0].request.pod, POD);
    assert_eq!(execs[0].request.container, "sandbox");
    assert_eq!(execs[0].request.command, argv::run_foreground("make test"));
    assert!(!execs[0].request.stdin);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_an_exec_that_ends_with_no_status_is_a_fault_and_never_an_exit_code() {
    let rig = rig();
    rig.cluster.push_script(Script::no_status());
    let error = rig
        .env
        .run(POD, &command("true"), RunMode::Foreground)
        .await
        .expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::NoStatus { .. })));
    assert!(is_fault(error));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_foreground_command_that_exceeds_the_exec_timeout_is_a_fault() {
    let rig = rig_with(json!({"exec_timeout_seconds": 5}));
    rig.cluster.push_script(Script::hanging());
    let error = rig
        .env
        .run(POD, &command("sleep 99"), RunMode::Foreground)
        .await
        .expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::TimedOut { .. })));
    assert!(is_fault(error));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_background_command_gives_the_process_id_as_decimal_text() {
    let rig = rig();
    rig.cluster.push_script(Script::out("4242\n"));
    let outcome = rig
        .env
        .run(POD, &command("sleep 99"), RunMode::Background)
        .await
        .expect("started");
    assert_eq!(outcome, RunOutcome::Started(ProcessId::new("4242")));
    assert_eq!(
        rig.cluster.execs()[0].request.command,
        argv::run_background("sleep 99")
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_background_start_with_no_process_id_is_a_fault() {
    for output in ["", "oops", "0", "-3"] {
        let rig = rig();
        rig.cluster.push_script(Script::out(output));
        let error = rig
            .env
            .run(POD, &command("sleep 99"), RunMode::Background)
            .await
            .expect_err("a fault");
        assert!(is_fault(error), "{output:?}");
    }
}
