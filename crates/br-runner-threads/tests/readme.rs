const README: &str = include_str!("../README.md");
const QUICKSTART: &str = include_str!("../examples/runner_wiring.rs");
const MANIFEST: &str = include_str!("../Cargo.toml");
const WORKSPACE: &str = include_str!("../../../Cargo.toml");

#[test]
fn the_quickstart_of_the_readme_is_the_runner_wiring_example_word_for_word() {
    assert!(
        README.contains(QUICKSTART.trim_end()),
        "README.md must hold examples/runner_wiring.rs as it is"
    );
}

#[test]
fn the_install_pin_names_the_version_of_the_crate() {
    let version = MANIFEST
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("the version of the crate");
    let pin = format!(
        "package = \"br-runner-threads\", tag = \"br-runner-threads/v{version}\", version = \"{version}\""
    );
    assert!(README.contains(&pin), "{pin}");
}

#[test]
fn the_install_block_pins_the_connection_with_the_tag_string_of_the_workspace() {
    let pin = WORKSPACE
        .lines()
        .find(|line| line.starts_with("br-runner-connection = "))
        .and_then(|line| line.split_once("package = "))
        .map(|(_, pin)| format!("package = {}", pin.trim_end_matches(" }")))
        .expect("the pin of br-runner-connection in the workspace");
    assert!(README.contains(&pin), "{pin}");
}
