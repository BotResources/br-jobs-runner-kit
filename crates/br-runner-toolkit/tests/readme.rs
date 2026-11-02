const README: &str = include_str!("../README.md");
const QUICKSTART: &str = include_str!("../examples/shared/serve.rs");
const SERVE_PUSH: &str = include_str!("../examples/serve_push.rs");
const SERVE_CLAIM: &str = include_str!("../examples/serve_claim.rs");
const MANIFEST: &str = include_str!("../Cargo.toml");
const WORKSPACE: &str = include_str!("../../../Cargo.toml");

#[test]
fn the_quickstart_of_the_readme_is_the_serve_code_of_the_examples_word_for_word() {
    for (name, code) in [
        ("examples/shared/serve.rs", QUICKSTART),
        ("examples/serve_push.rs", SERVE_PUSH),
        ("examples/serve_claim.rs", SERVE_CLAIM),
    ] {
        assert!(
            README.contains(code.trim_end()),
            "README.md must hold {name} as it is"
        );
    }
}

#[test]
fn the_install_pin_names_the_version_of_the_crate() {
    let version = MANIFEST
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("the version of the crate");
    let pin = format!(
        "package = \"br-runner-toolkit\", tag = \"br-runner-toolkit/v{version}\", version = \"{version}\""
    );
    assert!(README.contains(&pin), "{pin}");
}

fn install_block() -> Vec<(String, String)> {
    let after = README
        .split("## Install")
        .nth(1)
        .expect("the install section");
    let block = after
        .split("```toml")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("the install block");
    block
        .lines()
        .filter_map(|line| line.split_once(" = "))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect()
}

#[test]
fn every_git_pin_of_the_install_block_is_the_pin_the_workspace_builds_with() {
    for (name, value) in install_block()
        .iter()
        .filter(|(name, value)| name != "br-runner-toolkit" && value.contains("git = "))
    {
        let line = MANIFEST
            .lines()
            .chain(WORKSPACE.lines())
            .find(|line| line.starts_with(&format!("{name} = ")) && line.contains("git = "))
            .unwrap_or_else(|| panic!("{name} has no git pin in the manifests"));
        assert_eq!(line, format!("{name} = {value}"), "{name}");
    }
}

#[test]
fn the_install_block_holds_every_crate_the_quickstart_names() {
    let pinned: Vec<String> = install_block()
        .into_iter()
        .map(|(name, _)| name.replace('-', "_"))
        .collect();
    for ident in [
        "br_core_kernel",
        "br_runner_connection",
        "br_runner_jobs",
        "br_runner_toolkit",
        "tokio_util",
        "async_nats",
        "tokio",
    ] {
        assert!(QUICKSTART.contains(ident), "{ident}");
        assert!(pinned.contains(&ident.to_owned()), "{ident}");
    }
}

#[test]
fn the_install_block_holds_the_crate_that_names_the_passport_of_the_run_context() {
    assert!(
        install_block()
            .iter()
            .any(|(name, _)| name == "br-core-auth")
    );
}

#[test]
fn the_readme_names_every_builder_method_of_the_run_builder_and_of_the_test_host() {
    for method in [
        "with_queue_observer",
        "with_lifecycle_hook",
        "with_process_signals",
        "instance_lookup",
    ] {
        assert!(README.contains(method), "{method}");
    }
}
