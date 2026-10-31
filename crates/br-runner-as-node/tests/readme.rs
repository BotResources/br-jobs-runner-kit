const README: &str = include_str!("../README.md");
const QUICKSTART: &str = include_str!("../examples/call_a_runner_as_node.rs");
const MANIFEST: &str = include_str!("../Cargo.toml");
const WORKSPACE: &str = include_str!("../../../Cargo.toml");

#[test]
fn the_quickstart_of_the_readme_is_the_call_a_runner_as_node_example_word_for_word() {
    assert!(
        README.contains(QUICKSTART.trim_end()),
        "README.md must hold examples/call_a_runner_as_node.rs as it is"
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
        "package = \"br-runner-as-node\", tag = \"br-runner-as-node/v{version}\", version = \"{version}\""
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

fn line_of(manifest: &'static str, name: &str) -> Option<&'static str> {
    manifest
        .lines()
        .find(|line| line.starts_with(&format!("{name} = ")))
}

fn built_with(name: &str) -> &'static str {
    line_of(MANIFEST, name)
        .filter(|line| line.contains("git = "))
        .or_else(|| line_of(WORKSPACE, name))
        .unwrap_or_else(|| panic!("{name} is in neither the crate manifest nor the workspace"))
}

fn first_quoted(text: &str) -> &str {
    text.split('"').nth(1).expect("a quoted version")
}

#[test]
fn every_pin_of_the_install_block_is_the_pin_the_crate_is_built_with() {
    let pins = install_block();
    assert!(pins.len() >= 5, "{pins:?}");
    for (name, value) in pins.iter().filter(|(name, _)| name != "br-runner-as-node") {
        let line = built_with(name);
        if value.contains("git = ") {
            assert_eq!(line, format!("{name} = {value}"), "{name}");
        } else {
            let version = first_quoted(line.split_once(" = ").expect("a pin").1);
            assert!(
                value.contains(&format!("\"{version}\"")),
                "{name}: {value} does not hold version {version}"
            );
        }
    }
}

#[test]
fn the_install_block_holds_every_crate_the_quickstart_names() {
    let pinned: Vec<String> = install_block()
        .into_iter()
        .map(|(name, _)| name.replace('-', "_"))
        .collect();
    let mut named = Vec::new();
    for line in QUICKSTART.lines() {
        for (at, _) in line.match_indices("::") {
            let before = &line[..at];
            let start = before
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(0, |i| i + 1);
            let ident = &before[start..];
            let continues_a_path = before[..start].ends_with("::");
            if !continues_a_path && ident.starts_with(|c: char| c.is_ascii_lowercase()) {
                named.push(ident.to_owned());
            }
        }
    }
    named.retain(|ident| ident != "std");
    named.sort();
    named.dedup();
    assert!(named.len() >= 4, "{named:?}");
    for ident in named {
        assert!(
            pinned.contains(&ident),
            "{ident} is used by the quickstart and not pinned in the install block"
        );
    }
}
