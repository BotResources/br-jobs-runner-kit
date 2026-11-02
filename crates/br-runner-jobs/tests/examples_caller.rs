#[path = "../examples/call_a_runner.rs"]
#[allow(dead_code)]
// the example has its own main and helpers, unused when it is included as a module
mod call_a_runner;

use br_runner_jobs::test_double::{Answer, MemoryCall, memory_caller, result_ref};
use br_runner_jobs::{Content, SyncEnd};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn content(message: &str) -> Content {
    Content {
        message: message.to_owned(),
        attachments: Vec::new(),
    }
}

#[tokio::test]
async fn a_call_uploads_creates_waits_and_finishes_and_gives_the_inline_answer() {
    let (caller, script) = memory_caller();
    script.end(
        id(7),
        Answer::Now(SyncEnd::Completed {
            inline: Some(content("42 tickets")),
            result_ref: None,
        }),
    );

    let answer = call_a_runner::call_a_runner(&caller, id(7), analyst(), "count the tickets")
        .await
        .expect("an answer");

    assert_eq!(answer, "42 tickets");
    let calls = script.calls();
    assert!(
        matches!(&calls[0], MemoryCall::Upload { job_id, content: c } if *job_id == id(7) && c.message == "count the tickets")
    );
    assert!(
        matches!(&calls[1], MemoryCall::Create(input) if input.job_id == id(7) && input.runner_type == analyst())
    );
    assert!(matches!(calls.last(), Some(MemoryCall::Finish { job_id, .. }) if *job_id == id(7)));
}

#[tokio::test]
async fn a_call_whose_end_holds_a_result_reference_reads_the_result() {
    let (caller, script) = memory_caller();
    script.end(
        id(7),
        Answer::Now(SyncEnd::Completed {
            inline: None,
            result_ref: Some(result_ref(id(9))),
        }),
    );
    script.read_result(Answer::Now(content("from the result")));

    let answer = call_a_runner::call_a_runner(&caller, id(7), analyst(), "go")
        .await
        .expect("an answer");

    assert_eq!(answer, "from the result");
}

#[tokio::test]
async fn a_failed_or_cancelled_call_is_an_error_and_is_not_finished() {
    for end in [
        SyncEnd::Failed {
            report: None,
            message: "the runner failed".to_owned(),
        },
        SyncEnd::Cancelled {
            message: "the runner failed".to_owned(),
        },
    ] {
        let (caller, script) = memory_caller();
        script.end(id(7), Answer::Now(end));

        let error = call_a_runner::call_a_runner(&caller, id(7), analyst(), "go")
            .await
            .expect_err("an error");

        assert_eq!(error.to_string(), "the runner failed");
        assert!(
            !script
                .calls()
                .iter()
                .any(|call| matches!(call, MemoryCall::Finish { .. }))
        );
    }
}

fn analyst() -> br_runner_jobs::RunnerType {
    br_runner_jobs::RunnerType::new("analyst").expect("a runner type")
}
