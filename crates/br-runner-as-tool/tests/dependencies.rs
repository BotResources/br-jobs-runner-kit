const MANIFEST: &str = include_str!("../Cargo.toml");

fn dependency_names() -> Vec<String> {
    MANIFEST
        .lines()
        .filter(|line| line.contains(" = ") && !line.starts_with('['))
        .filter_map(|line| line.split(" = ").next())
        .map(|name| name.trim().to_string())
        .collect()
}

#[test]
fn the_crate_depends_on_the_jobs_client_and_not_on_the_jobs_adapters() {
    let names = dependency_names();
    assert!(names.iter().any(|name| name == "br-runner-jobs-client"));
    assert!(!names.iter().any(|name| name == "br-runner-jobs"));
}

#[test]
fn the_crate_pins_the_agentic_contract_by_its_tag() {
    assert!(MANIFEST.contains("tag = \"br-agentic-contract/v0.3.0\""));
    assert!(MANIFEST.contains("tag = \"br-runner-jobs-client/v0.1.1\""));
}
