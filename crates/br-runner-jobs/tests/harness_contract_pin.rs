const MANIFEST: &str = include_str!("../Cargo.toml");
const LOCK: &str = include_str!("../../../Cargo.lock");

const PIN: &str = "package = \"br-agentic-harness-contract\", tag = \"br-agentic-harness-contract/v0.2.0\", version = \"0.2.0\"";

#[test]
fn the_crate_depends_on_the_harness_contract_0_2_0() {
    let line = MANIFEST
        .lines()
        .find(|line| line.starts_with("br-agentic-harness-contract = "))
        .expect("the dependency on the harness contract");
    assert!(line.contains(PIN), "{line}");
}

#[test]
fn the_lock_resolves_the_harness_contract_0_2_0_for_the_crate() {
    let header = format!(
        "name = \"{}\"\nversion = \"{}\"\n",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    let package = LOCK
        .split("[[package]]\n")
        .find(|package| package.starts_with(&header))
        .expect("the lock entry of the crate");
    assert!(
        package
            .lines()
            .any(|line| line == " \"br-agentic-harness-contract 0.2.0\","),
        "{package}"
    );
}

#[test]
fn the_dependency_graph_holds_one_copy_of_the_agentic_contract() {
    let header = "name = \"br-agentic-contract\"\n";
    let copies: Vec<&str> = LOCK
        .split("[[package]]\n")
        .filter(|package| package.starts_with(header))
        .collect();
    assert_eq!(copies.len(), 1);
    assert!(
        copies[0].contains("br-agentic-contract%2Fv0.3.0"),
        "{}",
        copies[0]
    );
}
