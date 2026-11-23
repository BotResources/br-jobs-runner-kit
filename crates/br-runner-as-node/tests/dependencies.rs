#[test]
fn given_the_manifest_when_the_dependencies_are_read_then_only_the_jobs_client_of_the_kit_is_imported()
 {
    let manifest = include_str!("../Cargo.toml");
    assert!(manifest.contains("package = \"br-runner-jobs-client\""));
    assert!(!manifest.contains("package = \"br-runner-jobs\""));
    assert!(!manifest.contains("package = \"contract-jobs\""));
}

#[test]
fn given_the_runner_type_of_the_node_when_it_is_made_then_it_is_the_one_of_the_jobs_client() {
    let from_node: br_runner_as_node::RunnerType =
        br_runner_jobs_client::RunnerType::new("analyst");
    let _: br_runner_jobs_client::RunnerType = from_node;
}
