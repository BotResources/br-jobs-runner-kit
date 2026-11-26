use br_agentic_harness_contract::FilePath;
use br_runner_sandbox::{AbsolutePath, argv};

use crate::common::config;

fn absolute(text: &str) -> AbsolutePath {
    AbsolutePath::new("test", text).expect("a path")
}

fn texts(argv: &[String]) -> Vec<&str> {
    argv.iter().map(String::as_str).collect()
}

fn position(argv: &[String], item: &str) -> Option<usize> {
    argv.iter().position(|text| text == item)
}

#[test]
fn the_save_command_is_the_documented_create_call_over_the_save_scope() {
    let config = config();
    let command = argv::tar_create(config.tools_path(), config.save_scope_paths());
    assert_eq!(
        texts(&command),
        [
            "/opt/sbx/bin/bsdtar",
            "--create",
            "--file",
            "-",
            "--format",
            "pax",
            "--numeric-owner",
            "--no-xattrs",
            "--no-acls",
            "--no-fflags",
            "--options",
            "hdrcharset=BINARY",
            "-C",
            "/",
            "--",
            "work",
            "home/agent",
        ]
    );
}

#[test]
fn the_save_command_follows_the_configured_scope_and_tool_path() {
    let command = argv::tar_create(&absolute("/usr/local/bin/bsdtar"), &[absolute("/work")]);
    assert_eq!(
        command.first().map(String::as_str),
        Some("/usr/local/bin/bsdtar")
    );
    assert_eq!(command.last().map(String::as_str), Some("work"));
    assert!(!command.contains(&"home/agent".to_owned()));
}

#[test]
fn the_restore_command_extracts_from_the_root_keeping_owner_and_permissions() {
    let config = config();
    let command = argv::tar_restore(config.tools_path());
    assert_eq!(
        texts(&command),
        [
            "/opt/sbx/bin/bsdtar",
            "--extract",
            "--file",
            "-",
            "--preserve-permissions",
            "--same-owner",
            "--numeric-owner",
            "--no-xattrs",
            "--no-acls",
            "--no-fflags",
            "-C",
            "/",
        ]
    );
}

#[test]
fn the_write_command_extracts_into_the_work_folder_without_taking_the_owner() {
    let config = config();
    let command = argv::tar_write(config.tools_path());
    assert!(command.contains(&"--no-same-owner".to_owned()));
    assert!(!command.contains(&"--same-owner".to_owned()));
    let directory = position(&command, "-C").expect("a directory option");
    assert_eq!(command[directory + 1], "/work");
}

#[test]
fn the_seed_command_runs_the_seed_tool_in_the_seed_root_and_keeps_the_owner() {
    let config = config();
    let command = argv::tar_seed(config.seed_tools_path(), config.seed_root());
    assert_eq!(command.first().map(String::as_str), Some("/sbx/bin/bsdtar"));
    assert!(command.contains(&"--same-owner".to_owned()));
    assert!(command.contains(&"--numeric-owner".to_owned()));
    assert!(command.contains(&"--preserve-permissions".to_owned()));
    let directory = position(&command, "-C").expect("a directory option");
    assert_eq!(command[directory + 1], "/skills");
}

#[test]
fn every_tar_call_carries_the_three_metadata_refusals() {
    let config = config();
    let calls = [
        argv::tar_create(config.tools_path(), config.save_scope_paths()),
        argv::tar_restore(config.tools_path()),
        argv::tar_write(config.tools_path()),
        argv::tar_seed(config.seed_tools_path(), config.seed_root()),
    ];
    for call in calls {
        for flag in ["--no-xattrs", "--no-acls", "--no-fflags"] {
            assert!(call.contains(&flag.to_owned()), "{flag} in {call:?}");
        }
    }
}

#[test]
fn the_header_charset_option_is_on_create_only() {
    let config = config();
    let create = argv::tar_create(config.tools_path(), config.save_scope_paths());
    assert!(create.contains(&"hdrcharset=BINARY".to_owned()));
    let extracts = [
        argv::tar_restore(config.tools_path()),
        argv::tar_write(config.tools_path()),
        argv::tar_seed(config.seed_tools_path(), config.seed_root()),
    ];
    for extract in extracts {
        assert!(!extract.iter().any(|text| text.contains("hdrcharset")));
        assert!(!extract.contains(&"--options".to_owned()));
    }
}

#[test]
fn the_command_text_is_one_argument_and_is_never_spliced_into_the_script() {
    let text = "echo 'a b' \"$HOME\"; exit 3";
    for command in [argv::run_foreground(text), argv::run_background(text)] {
        assert_eq!(command.last().map(String::as_str), Some(text));
        assert_eq!(&command[..2], ["sh", "-c"]);
        assert!(!command[2].contains(text));
    }
}

#[test]
fn read_list_and_kill_name_the_work_folder_and_the_process_without_splicing() {
    let path = FilePath::new("notes/a.txt").expect("path");
    assert_eq!(texts(&argv::read(&path)), ["cat", "/work/notes/a.txt"]);
    let listed = argv::list(None);
    assert_eq!(listed.last().map(String::as_str), Some("/work"));
    let under = FilePath::new("notes").expect("path");
    let listed = argv::list(Some(&under));
    assert_eq!(listed.last().map(String::as_str), Some("/work/notes"));
    assert!(!listed[2].contains("/work"));
    let killed = argv::kill(4242);
    assert_eq!(killed.last().map(String::as_str), Some("4242"));
    assert!(!killed[2].contains("4242"));
}
