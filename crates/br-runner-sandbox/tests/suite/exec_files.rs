use std::io::Read;

use br_agentic_harness_contract::{Command, FilePath, PortError, ProcessId, RunMode};
use br_runner_sandbox::{ClusterError, Error, argv};
use tar::Archive;

use crate::fakes::{POD, Script, rig};

fn command(text: &str) -> Command {
    Command::new(text).expect("a command")
}

fn path(text: &str) -> FilePath {
    FilePath::new(text).expect("a path")
}

fn is_fault(error: Error) -> bool {
    !PortError::from(error).is_refusal()
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_read_gives_the_bytes_of_the_file() {
    let rig = rig();
    rig.cluster
        .push_script(Script::out(b"\x00binary\xff".to_vec()));
    let bytes = rig
        .env
        .read(POD, &path("notes/a.bin"))
        .await
        .expect("bytes");
    assert_eq!(bytes, b"\x00binary\xff");
    assert_eq!(
        rig.cluster.execs()[0].request.command,
        argv::read(&path("notes/a.bin"))
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_read_of_a_file_that_cannot_be_read_is_a_refusal_with_no_file_content() {
    let rig = rig();
    rig.cluster
        .push_script(Script::exit(1).with_stderr("cat: SECRET-STDERR: no such file"));
    let error = rig
        .env
        .read(POD, &path("missing.txt"))
        .await
        .expect_err("a refusal");
    let port = PortError::from(error);
    assert!(port.is_refusal());
    let reason = port.reason().expect("a reason").to_string();
    assert!(reason.contains("missing.txt"));
    assert!(!reason.contains("SECRET-STDERR"));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_read_with_no_status_is_a_fault_and_not_a_refusal() {
    let rig = rig();
    rig.cluster.push_script(Script::no_status());
    let error = rig
        .env
        .read(POD, &path("a.txt"))
        .await
        .expect_err("a fault");
    assert!(is_fault(error));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_write_sends_one_tar_entry_through_bsdtar_and_closes_the_input() {
    let rig = rig();
    rig.cluster.push_script(Script::like_bsdtar_extract());
    rig.env
        .write(POD, &path("deep/er/file.txt"), b"hello")
        .await
        .expect("written");
    let execs = rig.cluster.execs();
    let record = &execs[0];
    assert_eq!(
        record.request.command,
        argv::tar_write(rig.config.tools_path())
    );
    assert_eq!(record.request.container, "sandbox");
    assert!(record.request.stdin);
    assert!(record.stdin_closed());
    let sent = record.stdin();
    let mut archive = Archive::new(sent.as_slice());
    let mut entries = archive.entries().expect("entries");
    let mut entry = entries.next().expect("one entry").expect("readable");
    assert_eq!(
        entry.path().expect("path").to_string_lossy(),
        "deep/er/file.txt"
    );
    assert_eq!(entry.header().mode().expect("mode"), 0o644);
    let mut content = Vec::new();
    entry.read_to_end(&mut content).expect("content");
    assert_eq!(content, b"hello");
    assert!(entries.next().is_none());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_failed_write_is_a_fault_that_does_not_carry_the_standard_error() {
    let rig = rig();
    rig.cluster
        .push_script(Script::exit(1).with_stderr("bsdtar: SECRET-STDERR"));
    let error = rig
        .env
        .write(POD, &path("a.txt"), b"x")
        .await
        .expect_err("a fault");
    assert!(!error.to_string().contains("SECRET-STDERR"));
    assert!(is_fault(error));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_list_gives_relative_paths_in_ascending_byte_order_and_drops_invalid_names() {
    let rig = rig();
    rig.cluster.push_script(Script::out(
        b"/work/b.txt\0/work/Z.txt\0/work/a/c.txt\0/work/x\\y\0".to_vec(),
    ));
    let listed = rig.env.list(POD, None).await.expect("a list");
    let names: Vec<&str> = listed.iter().map(FilePath::as_str).collect();
    assert_eq!(names, ["Z.txt", "a/c.txt", "b.txt"]);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_list_of_a_folder_that_is_not_there_is_empty_and_a_find_failure_is_a_fault() {
    let rig = rig();
    rig.cluster.push_script(Script::out(Vec::new()));
    let under = path("nothing");
    assert_eq!(
        rig.env.list(POD, Some(&under)).await.expect("empty"),
        Vec::new()
    );
    assert_eq!(
        rig.cluster.execs()[0].request.command,
        argv::list(Some(&under))
    );
    rig.cluster.push_script(Script::exit(127));
    let error = rig.env.list(POD, None).await.expect_err("a fault");
    assert!(is_fault(error));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_kill_sends_the_process_id_it_was_given_and_nothing_else() {
    let rig = rig();
    rig.cluster.push_script(Script::exit(0));
    rig.env
        .kill(POD, &ProcessId::new("4242"))
        .await
        .expect("killed");
    assert_eq!(rig.cluster.execs()[0].request.command, argv::kill(4242));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_kill_refuses_a_text_that_is_not_a_process_id_before_any_exec() {
    for text in [
        "0",
        "-1",
        "abc",
        "12 3",
        "",
        "007",
        "1;reboot",
        "4294967296",
    ] {
        let rig = rig();
        let error = rig
            .env
            .kill(POD, &ProcessId::new(text))
            .await
            .expect_err("a refusal");
        assert!(PortError::from(error).is_refusal(), "{text:?}");
        assert!(rig.cluster.execs().is_empty(), "{text:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_pod_that_is_gone_is_a_fault_for_every_operation() {
    let gone = || Script {
        refuse: Some(ClusterError::Gone(POD.to_owned())),
        ..Script::default()
    };
    let rig = rig();
    for _ in 0..5 {
        rig.cluster.push_script(gone());
    }
    let run = rig
        .env
        .run(POD, &command("true"), RunMode::Foreground)
        .await;
    let read = rig.env.read(POD, &path("a")).await.map(|_| ());
    let write = rig.env.write(POD, &path("a"), b"x").await;
    let list = rig.env.list(POD, None).await.map(|_| ());
    let kill = rig.env.kill(POD, &ProcessId::new("7")).await;
    for error in [
        run.map(|_| ()).expect_err("fault"),
        read.expect_err("fault"),
        write.expect_err("fault"),
        list.expect_err("fault"),
        kill.expect_err("fault"),
    ] {
        assert!(matches!(error, Error::Cluster(ClusterError::Gone(_))));
        assert!(is_fault(error));
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_nothing_that_the_adapter_holds_is_sent_to_the_box() {
    let rig = rig();
    rig.cluster.push_script(Script::out("5\n"));
    rig.cluster.push_script(Script::out("x"));
    rig.cluster.push_script(Script::like_bsdtar_extract());
    rig.cluster.push_script(Script::out(Vec::new()));
    rig.cluster.push_script(Script::exit(0));
    rig.env
        .run(POD, &command("echo hi"), RunMode::Background)
        .await
        .expect("run");
    rig.env.read(POD, &path("a")).await.expect("read");
    rig.env.write(POD, &path("a"), b"x").await.expect("write");
    rig.env.list(POD, None).await.expect("list");
    rig.env.kill(POD, &ProcessId::new("5")).await.expect("kill");
    for record in rig.cluster.execs() {
        let sent = format!(
            "{:?} {}",
            record.request,
            String::from_utf8_lossy(&record.stdin())
        );
        for secret in [
            "AKIDEXAMPLE",
            "wJalrSECRETEXAMPLE",
            "store.example.test",
            "saves",
        ] {
            assert!(!sent.contains(secret), "{secret} in {sent}");
        }
        assert_eq!(record.request.container, "sandbox");
    }
}
