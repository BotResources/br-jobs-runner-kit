mod support;

use br_agentic_contract::PortError;
use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::test_double::CommandBody;
use support::*;

#[tokio::test]
async fn an_ai_turn_goes_to_threads_as_an_opening_chunks_and_a_sealed_closing() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);

    world
        .access()
        .add_entry(&thread, turn.clone(), notice("hello"))
        .await
        .expect("accepted");
    let entry = entry_id();
    world
        .access()
        .complete_response(&thread, turn.clone(), response_text(entry, "hi"))
        .await
        .expect("accepted");
    world
        .access()
        .close_turn(
            &thread,
            turn.clone(),
            TurnClosure::new(entry_id(), ClosureReason::completed()),
        )
        .await
        .expect("accepted");

    eventually("all delivered", || {
        world.threads.commands().len() == 2 && world.threads.chunks().len() >= 4
    })
    .await;
    let commands = world.threads.commands();
    assert!(matches!(commands[0].body, CommandBody::Store(_)));
    assert!(matches!(commands[1].body, CommandBody::Complete(_)));
    let seqs: Vec<u64> = world.threads.chunks().iter().map(|c| c.1).collect();
    assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());
    let _: Option<PortError> = None;
}
