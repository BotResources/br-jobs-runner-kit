const MANIFEST: &str = include_str!("../Cargo.toml");
const LOCK: &str = include_str!("../../../Cargo.lock");

const CONTRACT: &str = "br-agentic-harness-contract";
const CONTRACT_VERSION: &str = "0.2.0";
const GRAPH_GIT_URL: &str = "git+https://github.com/BotResources/br-llm-graph-rs?tag=";

const PIN: &str = "package = \"br-agentic-harness-contract\", tag = \"br-agentic-harness-contract/v0.2.0\", version = \"0.2.0\"";

fn lock_entries(name: &str) -> Vec<&'static str> {
    let header = format!("name = \"{name}\"\n");
    LOCK.split("[[package]]\n")
        .filter(|package| package.starts_with(&header))
        .collect()
}

fn lock_field<'a>(entry: &'a str, key: &str) -> &'a str {
    let prefix = format!("{key} = \"");
    entry
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
        .unwrap_or_else(|| panic!("the field {key} in {entry}"))
}

fn lock_entry_of_the_crate() -> &'static str {
    let header = format!(
        "name = \"{}\"\nversion = \"{}\"\n",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    LOCK.split("[[package]]\n")
        .find(|package| package.starts_with(&header))
        .expect("the lock entry of the crate")
}

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
    let package = lock_entry_of_the_crate();
    let dependency_prefix = format!(" \"{CONTRACT}");
    let suffix = package
        .lines()
        .find_map(|line| line.strip_prefix(&dependency_prefix)?.strip_suffix("\","))
        .unwrap_or_else(|| panic!("the dependency on the harness contract in {package}"));
    if suffix.is_empty() {
        let copies = lock_entries(CONTRACT);
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert_eq!(lock_field(copies[0], "version"), CONTRACT_VERSION);
    } else {
        assert_eq!(suffix, format!(" {CONTRACT_VERSION}"), "{package}");
    }
}

#[test]
fn the_dependency_graph_holds_one_copy_of_the_agentic_contract() {
    let copies = lock_entries("br-agentic-contract");
    assert_eq!(copies.len(), 1);
    assert!(
        copies[0].contains("br-agentic-contract%2Fv0.3.0"),
        "{}",
        copies[0]
    );
}

#[test]
fn the_harness_contract_and_the_agentic_contract_come_from_the_same_git_url() {
    let harness = lock_entries(CONTRACT)
        .into_iter()
        .find(|entry| lock_field(entry, "version") == CONTRACT_VERSION)
        .expect("the lock entry of the harness contract 0.2.0");
    let agentic = lock_entries("br-agentic-contract");
    assert_eq!(agentic.len(), 1);
    assert!(
        lock_field(harness, "source").starts_with(GRAPH_GIT_URL),
        "{harness}"
    );
    assert!(
        lock_field(agentic[0], "source").starts_with(GRAPH_GIT_URL),
        "{}",
        agentic[0]
    );
}
