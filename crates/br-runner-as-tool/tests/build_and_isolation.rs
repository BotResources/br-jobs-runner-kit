mod support;

use br_llm_clients_core::{BuildError, MAX_NESTING_DEPTH, Parameter, ParameterKind, Parameters};
use br_llm_messages::ToolName;
use br_runner_as_tool::{RunnerTool, RunnerToolError, ToolMode};
use br_runner_jobs::Error;
use br_runner_jobs::test_double::{Answer, memory_caller};
use serde_json::json;
use support::*;

fn deep_parameters() -> Parameters {
    let mut kind = ParameterKind::String;
    for _ in 0..=MAX_NESTING_DEPTH {
        let inner = Parameter::new("inner", "one level", true, kind).expect("a parameter");
        kind = ParameterKind::Object {
            parameters: Parameters::new(vec![inner]).expect("parameters"),
        };
    }
    let outer = Parameter::new("outer", "the top", true, kind).expect("a parameter");
    Parameters::new(vec![outer]).expect("parameters")
}

#[test]
fn given_a_name_a_description_and_a_schema_when_built_then_the_tool_spec_holds_them() {
    let (caller, _script) = memory_caller();
    let parameters = Parameters::new(vec![
        Parameter::new("question", "what to ask", true, ParameterKind::String).expect("parameter"),
    ])
    .expect("parameters");
    let tool = RunnerTool::new(
        "analyst",
        ToolName::new("ask_analyst").expect("name"),
        "Asks the analyst.",
        parameters.clone(),
        ToolMode::Sync,
        caller,
    )
    .build()
    .expect("a tool");

    assert_eq!(tool.name().as_str(), "ask_analyst");
    assert_eq!(tool.spec().description(), "Asks the analyst.");
    assert_eq!(tool.spec().parameters(), &parameters);
}

#[test]
fn given_a_schema_the_tool_spec_refuses_when_built_then_the_error_of_the_spec_comes_and_no_tool_exists()
 {
    let (caller, _script) = memory_caller();
    let result = RunnerTool::new(
        "analyst",
        ToolName::new("ask_analyst").expect("name"),
        "Asks the analyst.",
        deep_parameters(),
        ToolMode::Sync,
        caller,
    )
    .build();

    match result {
        Err(RunnerToolError::Spec(BuildError::NestingTooDeep { .. })) => {}
        other => panic!("{:?}", other.map(|tool| tool.name().as_str().to_owned())),
    }
}

#[test]
fn given_a_runner_type_the_wire_does_not_accept_when_built_then_invalid_input_and_no_tool_exists() {
    for bad in ["", "ana lyst", "ana.lyst", "*", ">", &"x".repeat(65)] {
        let (caller, _script) = memory_caller();
        let result = builder(bad, ToolMode::Sync, caller).build();
        assert!(
            matches!(
                result,
                Err(RunnerToolError::RunnerType(Error::InvalidInput(_)))
            ),
            "{bad:?}"
        );
    }
}

#[tokio::test]
async fn given_two_tools_with_two_handles_when_called_then_each_creates_through_its_own_handle_only()
 {
    let (caller_a, script_a) = memory_caller();
    let (caller_b, script_b) = memory_caller();
    let tool_a = builder("analyst", ToolMode::Sync, caller_a)
        .build()
        .expect("a tool");
    let tool_b = builder("writer", ToolMode::Sync, caller_b)
        .build()
        .expect("a tool");
    let call_a = spawn_call(&tool_a, json!({ "who": "a" }));
    let call_b = spawn_call(&tool_b, json!({ "who": "b" }));
    let create_a = create_of(&script_a).await;
    let create_b = create_of(&script_b).await;
    script_a.end(create_a.job_id, Answer::Now(completed("from a")));
    script_b.end(create_b.job_id, Answer::Now(completed("from b")));

    let out_a = call_a.finished().await.expect("a result");
    let out_b = call_b.finished().await.expect("a result");

    assert_eq!(texts(&out_a), vec!["from a".to_owned()]);
    assert_eq!(texts(&out_b), vec!["from b".to_owned()]);
    assert_eq!(create_a.runner_type, "analyst");
    assert_eq!(create_b.runner_type, "writer");
    assert_eq!(count(&script_a, is_create), 1);
    assert_eq!(count(&script_b, is_create), 1);
}
