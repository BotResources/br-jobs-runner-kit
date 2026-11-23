use contract_jobs::graphql_roots::PAYLOAD_ROOT;
use serde_json::json;

use super::*;
use crate::backend::{ChildCommand, LogRecord};
use crate::error::{Error, Operation};
use crate::model::{Content, LogLevel, ResultRef};
use crate::test_double::Call;

fn commands(jobs: &ScriptedJobs) -> Vec<ChildCommand> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Command(command) => Some(command),
            _ => None,
        })
        .collect()
}

fn logs(jobs: &ScriptedJobs) -> Vec<LogRecord> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Log(line) => Some(line),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn given_a_work_when_it_cancels_its_own_job_then_the_command_carries_the_command_id_the_job_and_the_run()
 {
    let jobs = jobs();
    let work = work(&jobs);
    work.cancel_own_job(id(50)).await.expect("accepted");
    assert_eq!(
        commands(&jobs),
        vec![ChildCommand::Cancel {
            command_id: id(50),
            job_id: id(JOB),
            run_id: id(RUN),
        }]
    );
}

#[tokio::test]
async fn given_a_refusal_when_the_work_cancels_its_own_job_then_refused_is_returned_with_the_reason()
 {
    let jobs = jobs();
    let work = work(&jobs);
    jobs.answer_commands(|| {
        Err(Error::refused(
            Operation::Cancel,
            "cancel_not_allowed",
            "no",
        ))
    });
    match work.cancel_own_job(id(50)).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.operation, Operation::Cancel);
            assert_eq!(refusal.reason, "cancel_not_allowed");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn given_a_cancel_sent_twice_with_one_command_id_when_the_work_repeats_it_then_the_same_id_goes_both_times_and_the_reply_of_jobs_is_passed_on()
 {
    let jobs = jobs();
    let work = work(&jobs);
    work.cancel_own_job(id(50)).await.expect("first");
    work.cancel_own_job(id(50)).await.expect("repeat");
    let sent = commands(&jobs);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], sent[1]);
}

#[tokio::test]
async fn given_a_work_when_it_logs_then_the_line_carries_the_run_the_level_the_step_and_the_id() {
    let jobs = jobs();
    let work = work(&jobs);
    for level in [LogLevel::Info, LogLevel::Warning, LogLevel::Error] {
        work.log(level, "text", None, None).await.expect("logged");
    }
    work.log(LogLevel::Info, "stepped", Some(3), Some(id(60)))
        .await
        .expect("logged");
    let lines = logs(&jobs);
    assert_eq!(
        lines.iter().map(|line| line.level).collect::<Vec<_>>(),
        vec![
            LogLevel::Info,
            LogLevel::Warning,
            LogLevel::Error,
            LogLevel::Info
        ]
    );
    assert!(lines.iter().all(|line| line.run_id == id(RUN)));
    assert_eq!((lines[0].step_index, lines[0].id), (None, None));
    assert_eq!(lines[3].message, "stepped");
    assert_eq!((lines[3].step_index, lines[3].id), (Some(3), Some(id(60))));
}

#[test]
fn given_the_three_levels_then_their_wire_words_are_the_wire_ones() {
    assert_eq!(LogLevel::Info.as_str(), "INFO");
    assert_eq!(LogLevel::Warning.as_str(), "WARNING");
    assert_eq!(LogLevel::Error.as_str(), "ERROR");
}

#[tokio::test]
async fn given_a_log_that_fails_to_send_when_logged_then_the_failure_is_returned_and_the_line_is_not_sent_again()
 {
    let jobs = jobs();
    let work = work(&jobs);
    jobs.fail_next_log(Error::unavailable("no listener"));
    let failed = work.log(LogLevel::Error, "text", None, None).await;
    assert!(matches!(failed, Err(Error::Unavailable(_))));
    assert_eq!(logs(&jobs).len(), 1);
}

#[tokio::test]
async fn given_a_work_when_it_reads_the_payload_the_saved_state_and_a_result_then_each_call_carries_its_own_job_and_run()
 {
    let jobs = jobs();
    let work = work(&jobs);
    jobs.push_graphql(Ok(json!({ PAYLOAD_ROOT: { "success": true, "reason": null,
        "message": null, "content": { "message": "m", "attachments": [] } } })));
    let payload = work.payload().await;
    jobs.push_graphql(Err(Error::refused(
        Operation::SavedState,
        "none",
        "none kept",
    )));
    let state = work.saved_state().await;
    jobs.push_graphql(Err(Error::refused(Operation::Result, "gone", "gone")));
    let result = work.read_result(ResultRef(id(70)), true).await;
    assert_eq!(
        payload,
        Ok(Content {
            message: "m".to_owned(),
            attachments: Vec::new()
        })
    );
    assert!(matches!(state, Err(Error::Refused(_))));
    assert!(matches!(result, Err(Error::Refused(_))));
    let variables: Vec<_> = jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql { variables, .. } => Some(variables.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(variables.len(), 3);
    for text in &variables {
        assert!(text.contains(&id(JOB).to_string()), "{text}");
        assert!(text.contains(&id(RUN).to_string()), "{text}");
        assert!(text.contains("instance-a"), "{text}");
        assert!(!text.contains(&id(USER).to_string()), "{text}");
        assert!(!text.contains("claim-value-ocelot"), "{text}");
    }
    assert!(variables[2].contains(&id(70).to_string()));
}
