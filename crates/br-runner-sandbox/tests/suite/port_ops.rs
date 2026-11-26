use br_agentic_harness_contract::{
    AlreadySaved, BoxId, Command, FilePath, PortError, ProcessId, RunMode, RunOutcome, SandboxPort,
};
use br_runner_sandbox::ClusterError;

use crate::fakes::{Boxes, Lineage, Script, boxes, fresh_request, warm_pod};
use crate::part_tar::sample_tar;

async fn open_box(boxes: &Boxes) -> (BoxId, Lineage) {
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();
    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");
    (opened.box_id, lineage)
}

fn path() -> FilePath {
    FilePath::new("a.txt").expect("a path")
}

async fn every_operation(boxes: &Boxes, id: &BoxId) -> Vec<Result<(), PortError>> {
    let command = Command::new("true").expect("a command");
    vec![
        boxes
            .sandbox
            .run(id, &command, RunMode::Foreground)
            .await
            .map(|_| ()),
        boxes.sandbox.read(id, &path()).await.map(|_| ()),
        boxes.sandbox.write(id, &path(), b"x".to_vec()).await,
        boxes.sandbox.list(id, None).await.map(|_| ()),
        boxes.sandbox.kill(id, &ProcessId::new("42")).await,
        boxes.sandbox.save(id).await.map(|_| ()),
    ]
}

#[tokio::test(start_paused = true)]
async fn operations_on_a_box_that_was_never_opened_are_port_faults_and_no_exec_is_made() {
    let boxes = boxes();

    for result in every_operation(&boxes, &BoxId::new("warm-main-zzz")).await {
        let error = result.expect_err("a fault");
        assert!(!error.is_refusal());
    }

    assert!(boxes.cluster.scripts.execs().is_empty());
}

#[tokio::test(start_paused = true)]
async fn operations_on_a_released_box_are_port_faults_and_no_exec_is_made() {
    let boxes = boxes();
    let (id, _) = open_box(&boxes).await;
    boxes.sandbox.release(&id).await.expect("released");

    for result in every_operation(&boxes, &id).await {
        let error = result.expect_err("a fault");
        assert!(!error.is_refusal());
    }

    assert!(boxes.cluster.scripts.execs().is_empty());
}

#[tokio::test(start_paused = true)]
async fn run_foreground_gives_the_exit_code_and_the_two_outputs_of_the_box_pod_in_the_agent_container()
 {
    let boxes = boxes();
    let (id, _) = open_box(&boxes).await;
    boxes.cluster.scripts.push_script(Script {
        stdout: b"out".to_vec(),
        stderr: b"err".to_vec(),
        code: Some(3),
        ..Script::default()
    });
    let command = Command::new("make").expect("a command");

    let outcome = boxes
        .sandbox
        .run(&id, &command, RunMode::Foreground)
        .await
        .expect("a result");

    let RunOutcome::Finished(result) = outcome else {
        panic!("not finished");
    };
    assert_eq!(
        (result.exit_code, result.stdout, result.stderr),
        (3, b"out".to_vec(), b"err".to_vec())
    );
    let execs = boxes.cluster.scripts.execs();
    assert_eq!(execs[0].request.pod, "warm-main-a");
    assert_eq!(execs[0].request.container, "sandbox");
}

#[tokio::test(start_paused = true)]
async fn read_of_a_missing_file_is_a_port_refusal() {
    let boxes = boxes();
    let (id, _) = open_box(&boxes).await;
    boxes.cluster.scripts.push_script(Script::exit(1));

    let error = boxes.sandbox.read(&id, &path()).await.expect_err("refused");

    assert!(error.is_refusal());
}

#[tokio::test(start_paused = true)]
async fn an_exec_stream_that_ends_with_no_status_is_a_port_fault() {
    let boxes = boxes();
    let (id, _) = open_box(&boxes).await;
    boxes.cluster.scripts.push_script(Script::no_status());

    let error = boxes.sandbox.list(&id, None).await.expect_err("a fault");

    assert!(!error.is_refusal());
}

#[tokio::test(start_paused = true)]
async fn when_the_pod_is_gone_an_operation_is_a_port_fault_and_the_pod_is_not_made_again() {
    let boxes = boxes();
    let (id, _) = open_box(&boxes).await;
    boxes.cluster.scripts.push_script(Script {
        refuse: Some(ClusterError::Gone("warm-main-a".to_owned())),
        ..Script::default()
    });

    let error = boxes.sandbox.list(&id, None).await.expect_err("a fault");

    assert!(!error.is_refusal());
    assert!(boxes.cluster.created().is_empty());
}

#[tokio::test(start_paused = true)]
async fn save_gives_the_receipt_of_the_turn_and_a_second_save_of_the_turn_is_the_already_saved_refusal()
 {
    let boxes = boxes();
    let (id, _) = open_box(&boxes).await;
    boxes.cluster.scripts.push_script(Script::out(sample_tar()));

    let receipt = boxes.sandbox.save(&id).await.expect("a receipt");
    boxes.cluster.scripts.push_script(Script::out(sample_tar()));
    let second = boxes.sandbox.save(&id).await.expect_err("refused");

    assert_eq!(receipt.bytes, sample_tar().len() as u64);
    assert!(AlreadySaved::is(&second));
    assert!(second.is_refusal());
}

#[tokio::test(start_paused = true)]
async fn nothing_the_adapter_holds_reaches_the_box_no_credential_no_store_address() {
    let boxes = boxes();
    let (id, _) = open_box(&boxes).await;
    boxes.cluster.scripts.push_script(Script::out(sample_tar()));
    boxes.sandbox.save(&id).await.expect("a receipt");
    boxes
        .cluster
        .scripts
        .push_script(Script::like_bsdtar_extract());
    boxes.sandbox.write(&id, &path(), b"x".to_vec()).await.ok();

    for exec in boxes.cluster.scripts.execs() {
        let shown = format!("{:?}", exec.request);
        for secret in [
            "wJalrSECRETEXAMPLE",
            "AKIDEXAMPLE",
            "store.example.test",
            "saves",
        ] {
            assert!(!shown.contains(secret), "{shown}");
        }
    }
}
