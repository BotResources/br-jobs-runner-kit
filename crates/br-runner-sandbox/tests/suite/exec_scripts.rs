use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use br_agentic_harness_contract::FilePath;
use br_runner_sandbox::{argv, parse_find_output};
use uuid::Uuid;

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sbx-scripts-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("a folder");
        Self(fs::canonicalize(&path).expect("a canonical folder"))
    }

    fn text(&self) -> &str {
        self.0.to_str().expect("a text path")
    }

    fn touch(&self, relative: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("parents");
        fs::write(path, b"x").expect("a file");
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn local(command: &[String], folder: &Folder) -> Command {
    let adapted: Vec<String> = command
        .iter()
        .map(|part| part.replace("/work", folder.text()))
        .collect();
    let mut process = Command::new(&adapted[0]);
    process.args(&adapted[1..]);
    process
}

fn run(command: &[String], folder: &Folder) -> Output {
    local(command, folder).output().expect("the shell runs")
}

fn as_seen_in_the_pod(output: &Output, folder: &Folder) -> Vec<u8> {
    String::from_utf8_lossy(&output.stdout)
        .replace(folder.text(), "/work")
        .into_bytes()
}

fn gone_process_id() -> u32 {
    let mut child = Command::new("true").spawn().expect("a child");
    let id = child.id();
    child.wait().expect("the child ends");
    id
}

#[test]
fn a_foreground_command_runs_in_the_work_folder_with_its_own_exit_code_and_two_streams() {
    let folder = Folder::new();
    let command = argv::run_foreground("pwd; printf out; printf err >&2; exit 3");
    let output = run(&command, &folder);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\nout", folder.text())
    );
    assert_eq!(output.stderr, b"err");
}

#[test]
fn a_command_with_quotes_and_expansions_reaches_the_shell_as_written() {
    let folder = Folder::new();
    let command = argv::run_foreground("printf '%s' \"it's \\$HOME\"");
    let output = run(&command, &folder);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"it's $HOME");
}

#[test]
fn a_foreground_command_in_a_missing_work_folder_is_a_failed_command_not_a_success() {
    let folder = Folder::new();
    let mut command = argv::run_foreground("echo ran");
    command[2] = command[2].replace("/work", &format!("{}/missing", folder.text()));
    let output = Command::new(&command[0])
        .args(&command[1..])
        .output()
        .expect("the shell runs");
    assert_ne!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn a_background_command_gives_its_process_id_at_once_and_goes_on_after_the_shell_ends() {
    let folder = Folder::new();
    let command = argv::run_background("sleep 30");
    let output = run(&command, &folder);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let process: u32 = text.parse().expect("a decimal process id");
    let alive = Command::new("kill")
        .args(["-0", &process.to_string()])
        .status()
        .expect("kill runs");
    let killed = run(&argv::kill(process), &folder);
    assert!(alive.success(), "the background process outlives the shell");
    assert_eq!(killed.status.code(), Some(0));
}

#[test]
fn a_background_command_does_not_hold_the_output_streams_of_the_exec() {
    let folder = Folder::new();
    let command = argv::run_background("sleep 30");
    let mut child = local(&command, &folder)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the shell runs");
    let status = child.wait().expect("the shell ends");
    assert!(status.success());
    let mut text = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().expect("stdout"), &mut text)
        .expect("stdout ends at once");
    let process: u32 = text.trim().parse().expect("a process id");
    run(&argv::kill(process), &folder);
}

#[test]
fn kill_sends_sigkill_to_the_process_and_a_process_that_ended_is_not_an_error() {
    let folder = Folder::new();
    let mut child = Command::new("sleep").arg("30").spawn().expect("a child");
    let killed = run(&argv::kill(child.id()), &folder);
    assert_eq!(killed.status.code(), Some(0));
    assert_eq!(child.wait().expect("ended").signal(), Some(9));
    let again = run(&argv::kill(gone_process_id()), &folder);
    assert_eq!(again.status.code(), Some(0));
}

#[test]
fn list_prints_the_regular_files_under_a_folder_and_nothing_else() {
    let folder = Folder::new();
    folder.touch("b.txt");
    folder.touch("a/deep/c.txt");
    folder.touch("a/Z.txt");
    fs::create_dir_all(folder.0.join("empty")).expect("an empty folder");
    let output = run(&argv::list(None), &folder);
    assert_eq!(output.status.code(), Some(0));
    let listed = parse_find_output(0, &as_seen_in_the_pod(&output, &folder)).expect("a list");
    let names: Vec<&str> = listed.iter().map(FilePath::as_str).collect();
    assert_eq!(names, ["a/Z.txt", "a/deep/c.txt", "b.txt"]);
}

#[test]
fn list_under_a_folder_gives_paths_relative_to_the_work_folder() {
    let folder = Folder::new();
    folder.touch("a/deep/c.txt");
    folder.touch("b.txt");
    let under = FilePath::new("a").expect("path");
    let output = run(&argv::list(Some(&under)), &folder);
    let listed = parse_find_output(0, &as_seen_in_the_pod(&output, &folder)).expect("a list");
    let names: Vec<&str> = listed.iter().map(FilePath::as_str).collect();
    assert_eq!(names, ["a/deep/c.txt"]);
}

#[test]
fn list_of_a_folder_that_does_not_exist_gives_an_empty_list_and_exit_zero() {
    let folder = Folder::new();
    let under = FilePath::new("nothing/here").expect("path");
    let output = run(&argv::list(Some(&under)), &folder);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(parse_find_output(0, &output.stdout), Ok(Vec::new()));
}

#[test]
fn list_keeps_a_name_with_a_newline_whole_and_leaves_out_a_name_that_is_not_a_file_path() {
    let folder = Folder::new();
    folder.touch("line\nbreak.txt");
    folder.touch("back\\slash.txt");
    folder.touch("plain.txt");
    let output = run(&argv::list(None), &folder);
    let listed = parse_find_output(0, &as_seen_in_the_pod(&output, &folder)).expect("a list");
    let names: Vec<&str> = listed.iter().map(FilePath::as_str).collect();
    assert_eq!(names, ["line\nbreak.txt", "plain.txt"]);
}
