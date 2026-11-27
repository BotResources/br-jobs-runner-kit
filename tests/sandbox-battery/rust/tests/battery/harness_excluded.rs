use std::collections::BTreeSet;
use std::path::PathBuf;

use battery::Session;
use battery::error::{Res, data};
use battery::evidence::Checks;

const LIST: &str = include_str!("harness_excluded.txt");
const LOCK: &str = include_str!("../../Cargo.lock");

fn rows() -> Vec<Vec<&'static str>> {
    LIST.lines().map(|line| line.split('|').collect()).collect()
}

fn pinned_revision(package: &str) -> Res<String> {
    let header = format!("name = \"{package}\"");
    let block = LOCK
        .split("[[package]]")
        .find(|block| block.lines().any(|line| line == header))
        .ok_or_else(|| data("Cargo.lock", format!("no package {package}")))?;
    let source = block
        .lines()
        .find_map(|line| line.strip_prefix("source = \""))
        .ok_or_else(|| data("Cargo.lock", format!("no source for {package}")))?;
    let revision = source
        .rsplit('#')
        .next()
        .map(|sha| sha.trim_end_matches('"'))
        .ok_or_else(|| data("Cargo.lock", format!("no revision for {package}")))?;
    Ok(revision.chars().take(7).collect())
}

fn checkouts() -> Res<Vec<PathBuf>> {
    let home = match battery::env::optional("CARGO_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(battery::env::required("HOME", "the shell")?).join(".cargo"),
    };
    let root = home.join("git").join("checkouts");
    let listing = std::fs::read_dir(&root)
        .map_err(|error| data(format!("the folder {}", root.display()), error.to_string()))?;
    Ok(listing
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect())
}

fn upstream_source(package: &str, file: &str) -> Res<String> {
    let revision = pinned_revision(package)?;
    for checkout in checkouts()? {
        let path = checkout
            .join(&revision)
            .join("crates")
            .join(package)
            .join("tests")
            .join("harness")
            .join(file);
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Ok(text);
        }
    }
    Err(data(
        format!("the upstream test file {file} of {package}"),
        format!("not found in the cargo git checkouts at revision {revision}"),
    ))
}

#[tokio::test]
async fn the_upstream_tests_that_need_the_memory_sandbox_are_listed_with_their_reason() -> Res {
    let session = Session::start().await?;
    let mut checks = Checks::new(&session.env);
    let all = rows();
    let harness = all
        .iter()
        .filter(|row| row[0] == "br-agentic-harness")
        .count();
    let contract = all
        .iter()
        .filter(|row| row[0] == "br-agentic-harness-contract")
        .count();
    let complete = !all.is_empty()
        && harness > 0
        && contract > 0
        && all
            .iter()
            .all(|row| row.len() == 4 && !row[2].is_empty() && !row[3].is_empty());
    checks.expect(
        "harness-suite-every-excluded-upstream-test-has-a-name-and-a-reason",
        complete,
        format!("{harness} excluded in the harness suite and {contract} in the contract suite"),
    );
    let names: BTreeSet<&str> = all.iter().filter_map(|row| row.get(2).copied()).collect();
    checks.expect(
        "harness-suite-no-upstream-test-is-listed-twice",
        names.len() == all.len(),
        format!("{} rows, {} distinct names", all.len(), names.len()),
    );
    let mut missing = Vec::new();
    for row in all.iter().filter(|row| row.len() == 4) {
        let source = upstream_source(row[0], row[1])?;
        if !source.contains(&format!("fn {}(", row[2])) {
            missing.push(format!("{} {} {}", row[0], row[1], row[2]));
        }
    }
    checks.expect(
        "harness-suite-every-excluded-upstream-test-exists-upstream",
        missing.is_empty(),
        format!("not found in the pinned upstream source: {missing:?}"),
    );
    checks.finish()
}
