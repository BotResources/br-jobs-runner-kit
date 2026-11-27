use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

mod bite;
mod classes;
mod tree;

use classes::{ALLOWED, DIRECT, class_of};
use tree::Tree;

fn manifest_path() -> String {
    format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"))
}

fn metadata() -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .arg("--manifest-path")
        .arg(manifest_path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn keeper_tree() -> Tree {
    Tree::from_metadata(&metadata(), &manifest_path())
}

#[test]
fn the_normal_dependency_tree_of_the_keeper_holds_no_client_outside_the_allowed_list() {
    let tree = keeper_tree();
    let reached = tree.reach();
    assert!(reached.contains_key("kube"), "the walk found no kube");
    assert!(
        reached.contains_key("object_store"),
        "the walk found no object_store"
    );
    assert!(
        reached.len() > 100,
        "the walk found {} crates",
        reached.len()
    );
    assert_eq!(tree.network_violations(), Vec::<String>::new());
}

#[test]
fn the_direct_dependencies_of_the_keeper_are_the_closed_list() {
    assert_eq!(keeper_tree().direct_violations(), Vec::<String>::new());
}

#[test]
fn each_allowed_crate_is_in_a_class_and_in_the_tree_today() {
    let reached: BTreeSet<String> = keeper_tree().reach().into_keys().collect();
    for (name, role) in ALLOWED {
        assert!(class_of(name).is_some(), "{name} ({role}) is in no class");
        assert!(reached.contains(name), "{name} ({role}) is not in the tree");
    }
}

#[test]
fn the_closed_list_of_direct_dependencies_has_no_duplicate() {
    let unique: BTreeSet<&str> = DIRECT.iter().copied().collect();
    assert_eq!(unique.len(), DIRECT.len());
}
