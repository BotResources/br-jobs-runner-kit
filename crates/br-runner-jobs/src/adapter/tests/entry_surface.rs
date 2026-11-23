use std::fs;
use std::path::Path;

use br_agentic_contract::AgentPort;

use super::{Snap, World, instance, label};
use crate::adapter::mark_gate_stored;

fn adapter_source(file: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/adapter")
            .join(file),
    )
    .expect("a source file")
}

#[tokio::test]
async fn given_a_book_and_a_paused_jobs_when_the_node_entry_makes_its_ports_and_the_agent_id_then_jobs_gets_no_call_of_the_entry()
 {
    let world = World::new(mark_gate_stored()).await;
    world.registered_agent().await;
    let before = world.jobs.calls().len();
    world.jobs.pause();
    let entry = world.entry.clone();
    let _agent_port = entry.agent_port();
    let _incoming = entry.incoming_port();
    let _mail = entry.mail_port();
    let agent = entry
        .agent_id(&world.book, &label())
        .expect("the agent id at once");
    let again = entry
        .agent_port()
        .agent(&Snap, instance(), &label())
        .await
        .expect("the agent at once");
    assert_eq!(agent, again);
    super::yields().await;
    assert_eq!(world.jobs.calls().len(), before);
    world.jobs.resume();
}

#[tokio::test]
async fn given_the_two_ways_to_make_an_entry_then_each_gives_its_own_adapter_and_the_other_none() {
    let with_gate = World::new(mark_gate_stored()).await;
    assert!(with_gate.entry.incoming_port().is_some());
    assert!(with_gate.entry.mail_port().is_none());
    let for_harness = World::mail().await;
    assert!(for_harness.entry.incoming_port().is_none());
    assert!(for_harness.entry.mail_port().is_some());
}

#[tokio::test]
async fn given_a_node_entry_without_an_inbox_then_it_gives_the_agent_port_and_no_other_adapter() {
    let world = World::new(mark_gate_stored()).await;
    let plain = crate::adapter::NodeEntry::new("node", std::sync::Arc::clone(&world.lookup));
    assert!(plain.incoming_port().is_none() && plain.mail_port().is_none());
    let agent = plain
        .agent_port()
        .agent(&Snap, instance(), &id_label())
        .await
        .expect("an agent");
    assert!(!world.book.holds_inbox(agent.as_uuid()));
}

fn id_label() -> br_agentic_contract::AgentLabel {
    br_agentic_contract::AgentLabel::new("reader").expect("a label")
}

#[test]
fn given_the_sources_when_the_constructors_of_the_adapters_are_read_then_none_is_public_and_the_library_exports_only_the_named_items()
 {
    for (file, constructor) in [
        ("agent_port.rs", "fn new(shared"),
        ("incoming_port/mod.rs", "fn new(shared"),
        ("mail_port/mod.rs", "fn new(shared"),
    ] {
        let source = adapter_source(file);
        let line = source
            .lines()
            .find(|line| line.contains(constructor))
            .expect("a constructor");
        assert!(
            line.trim_start().starts_with("pub(super) fn new"),
            "{file}: {line}"
        );
    }
    let lib = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("lib.rs");
    let exported: Vec<&str> = lib
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|word| !word.is_empty() && !["mod", "adapter", "pub", "use"].contains(word))
        .collect();
    assert_eq!(
        exported,
        vec![
            "AgentPortAdapter",
            "GateFuture",
            "IncomingPortAdapter",
            "MailPortAdapter",
            "MarkGate",
            "NodeEntry",
            "mark_gate",
            "mark_gate_stored"
        ]
    );
}
