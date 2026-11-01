mod support;

use std::sync::{Arc, Mutex};

use br_runner_jobs::LogLevel;
use br_runner_jobs::test_double::{Call, ScriptedJobs};
use br_runner_toolkit::{RunRefusal, RunnerBuilder, StartArgs, StartedRun, run_start};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

fn sync_trigger(n: u128) -> Vec<u8> {
    call_with(trigger(n), &[("mode", json!("SYNC"))])
}

fn replies() -> serde_json::Value {
    let mut reply = json!({
        "jobsUnread": { "success": true, "items": [] },
        "jobsPayload": {
            "success": true,
            "content": { "message": "the payload", "attachments": [] }
        }
    });
    reply
        .as_object_mut()
        .expect("object")
        .extend(writes_ok().as_object().expect("object").clone());
    reply
}

#[tokio::test(start_paused = true)]
async fn given_a_run_start_function_when_it_reads_the_payload_then_the_content_comes_from_the_payload_call_of_jobs_for_this_run()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(replies());
    jobs.push_trigger(trigger(1));
    let payload: Arc<Mutex<Option<String>>> = Arc::default();
    let seen = Arc::clone(&payload);
    let running = run(
        &jobs,
        |b| b,
        1,
        StartArgs::new(
            single(Behavior::Result("x")),
            run_start(move |input| {
                let seen = Arc::clone(&seen);
                async move {
                    let content = input.context.payload().await.expect("a payload");
                    *seen.lock().expect("lock") = Some(content.message);
                    Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()]))
                }
            }),
            false,
        ),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(
        payload.lock().expect("lock").clone(),
        Some("the payload".to_owned())
    );
    let reads: Vec<serde_json::Value> = jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } if document.contains("jobsPayload") => Some(variables),
            _ => None,
        })
        .collect();
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0]["work"]["runId"], json!(id(101)));
    assert_eq!(reads[0]["work"]["jobId"], json!(id(201)));
}

#[tokio::test(start_paused = true)]
async fn given_a_run_with_a_passport_when_it_runs_with_an_answer_a_saved_state_and_a_log_line_then_no_passport_text_is_in_the_calls_to_jobs_or_in_the_library_log()
 {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(replies());
    jobs.push_trigger(sync_trigger(1));
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: true,
        seen_run: Default::default(),
    };
    let args = StartArgs::new(
        agent_graph(Behavior::Result("x"), agent),
        run_start(|input| async move {
            input
                .context
                .log(LogLevel::Info, "a line of the run", Some(0))
                .await
                .expect("logged");
            Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()]))
        }),
        true,
    );
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(args.with_shutdown(shutdown.clone())));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
    let calls = format!("{:?}", jobs.calls());
    for sent in [
        "a line of the run",
        "jobsSaveState",
        "jobsUploadResult",
        "Completed",
    ] {
        assert!(calls.contains(sent), "{sent}");
    }
    let sent_trigger = String::from_utf8(sync_trigger(1)).expect("text");
    let log_text = format!("{:?}", logs.lines());
    for secret in ["ocelot@example.test", id(900).to_string().as_str()] {
        assert!(sent_trigger.contains(secret), "{secret}");
        assert!(!calls.contains(secret), "{secret}");
        assert!(!log_text.contains(secret), "{secret}");
    }
}
