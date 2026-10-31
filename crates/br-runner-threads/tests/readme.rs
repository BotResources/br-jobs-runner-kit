const README: &str = include_str!("../README.md");
const QUICKSTART: &str = include_str!("../examples/runner_wiring.rs");
const MANIFEST: &str = include_str!("../Cargo.toml");

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
