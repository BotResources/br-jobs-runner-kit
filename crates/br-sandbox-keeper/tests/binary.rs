use std::process::Command;

use serde_json::Value;

fn run_without_configuration() -> (std::process::ExitStatus, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_br-sandbox-keeper"))
        .env_clear()
        .output()
        .unwrap();
    (
        output.status,
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_binary_writes_each_log_line_as_a_json_object_and_fails_without_configuration() {
    let (status, stdout, stderr) = run_without_configuration();
    assert!(!status.success());
    let lines: Vec<&str> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| !line.is_empty())
        .collect();
    assert!(!lines.is_empty());
    for line in &lines {
        assert!(line.starts_with('{'), "{line}");
        let parsed: Value = serde_json::from_str(line).unwrap();
        assert!(parsed.is_object(), "{line}");
    }
    let stopped: Value = serde_json::from_str(lines[lines.len() - 1]).unwrap();
    assert_eq!(stopped["level"], "ERROR");
    assert_eq!(stopped["fields"]["message"], "the keeper stopped");
    assert!(
        stopped["fields"]["error"]
            .as_str()
            .unwrap()
            .contains("BR_SANDBOX_CONFIG")
    );
}
