use std::path::PathBuf;

use super::*;

pub(crate) fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("battery-evidence-{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(dir.join("checks.tsv"));
    dir
}

fn verdicts(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("checks.tsv"))
        .unwrap_or_default()
        .lines()
        .map(|line| line.split('\t').nth(1).unwrap_or("").to_owned())
        .collect()
}

#[test]
fn a_test_with_no_check_and_no_skip_fails_loud() {
    let checks = Checks::in_dir(scratch_dir("none"));
    assert!(matches!(checks.finish(), Err(BatteryError::NoChecks)));
}

#[test]
fn a_skip_is_written_as_skip_and_counts_as_no_pass() {
    let dir = scratch_dir("skip-only");
    let mut checks = Checks::in_dir(dir.clone());
    checks.skip("case", "not for this chart");
    assert_eq!(
        checks.summary(),
        "summary: 0 pass, 0 fail, 1 skip: skipped, nothing passed"
    );
    assert!(checks.finish().is_ok());
    assert_eq!(verdicts(&dir), ["skip"]);
}

#[test]
fn a_pass_with_a_skip_reports_both_numbers() {
    let dir = scratch_dir("pass-skip");
    let mut checks = Checks::in_dir(dir.clone());
    checks.expect("one", true, "ok");
    checks.skip("two", "reason");
    assert_eq!(checks.summary(), "summary: 1 pass, 0 fail, 1 skip: passed");
    assert!(checks.finish().is_ok());
    assert_eq!(verdicts(&dir), ["pass", "skip"]);
}

#[test]
fn a_fail_beside_a_skip_still_fails_the_test() {
    let mut checks = Checks::in_dir(scratch_dir("fail-skip"));
    checks.skip("two", "reason");
    checks.expect("one", false, "bad");
    assert!(checks.summary().ends_with("failed"));
    assert!(matches!(
        checks.finish(),
        Err(BatteryError::Failed { count: 1, .. })
    ));
}
