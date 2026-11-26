use std::io::Read;

use br_agentic_harness_contract::{FilePath, PortError, Seed, SeedEntry};
use br_runner_sandbox::{Error, ExecError, PodKeys, argv, seed_hash_patch, seed_hash_text};
use serde_json::json;
use tar::Archive;

use crate::fakes::{POD, Script, rig, rig_with};

fn seed() -> Seed {
    let path = |text: &str| FilePath::new(text).expect("path");
    Seed::new(vec![
        SeedEntry::directory(path("review"), 0o775),
        SeedEntry::file(path("review/SKILL.md"), 0o664, b"# skill".to_vec()),
        SeedEntry::file(path("review/run.sh"), 0o755, b"#!/bin/sh\n".to_vec()),
    ])
    .expect("a seed")
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_the_seed_goes_through_the_seed_container_and_the_hash_is_set_after_it() {
    let rig = rig();
    let seed = seed();
    rig.cluster.push_script(Script::like_bsdtar_extract());
    rig.env.write_seed(POD, &seed).await.expect("a seed write");
    let execs = rig.cluster.execs();
    assert_eq!(execs.len(), 1);
    assert_eq!(execs[0].request.container, "seed");
    assert_eq!(
        execs[0].request.command,
        argv::tar_seed(rig.config.seed_tools_path(), rig.config.seed_root())
    );
    assert!(execs[0].request.stdin);
    assert!(execs[0].stdin_closed());
    let patches = rig.cluster.patches();
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].0, POD);
    assert_eq!(
        patches[0].1,
        seed_hash_patch(&PodKeys::default(), &seed_hash_text(&seed))
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_the_seed_stream_has_owner_zero_the_write_bits_removed_and_paths_under_the_root()
 {
    let rig = rig();
    rig.cluster.push_script(Script::like_bsdtar_extract());
    rig.env
        .write_seed(POD, &seed())
        .await
        .expect("a seed write");
    let sent = rig.cluster.execs()[0].stdin();
    let mut archive = Archive::new(sent.as_slice());
    let mut seen = Vec::new();
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("data");
        seen.push((
            entry
                .path()
                .expect("path")
                .to_string_lossy()
                .trim_end_matches('/')
                .to_owned(),
            entry.header().mode().expect("mode"),
            entry.header().uid().expect("uid"),
            entry.header().gid().expect("gid"),
        ));
    }
    assert_eq!(
        seen,
        [
            ("review".to_owned(), 0o555, 0, 0),
            ("review/SKILL.md".to_owned(), 0o444, 0, 0),
            ("review/run.sh".to_owned(), 0o555, 0, 0),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_an_empty_seed_writes_no_file_and_still_sets_the_hash_of_the_empty_set() {
    let rig = rig();
    let empty = Seed::new(Vec::new()).expect("an empty seed");
    rig.env.write_seed(POD, &empty).await.expect("a seed write");
    assert!(rig.cluster.execs().is_empty());
    let patches = rig.cluster.patches();
    assert_eq!(patches.len(), 1);
    assert_eq!(
        patches[0].1,
        seed_hash_patch(&PodKeys::default(), &seed_hash_text(&empty))
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_failed_seed_exec_is_a_fault_and_the_hash_is_not_set() {
    for script in [
        Script::exit(1).with_stderr("SECRET-STDERR"),
        Script::no_status(),
    ] {
        let rig = rig();
        rig.cluster.push_script(Script {
            wait_for_stdin_close: true,
            ..script
        });
        let error = rig.env.write_seed(POD, &seed()).await.expect_err("a fault");
        assert!(!error.to_string().contains("SECRET-STDERR"));
        assert!(!PortError::from(error).is_refusal());
        assert!(rig.cluster.patches().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_seed_exec_that_exceeds_the_exec_timeout_is_a_fault_and_the_hash_is_not_set()
{
    let rig = rig_with(json!({"exec_timeout_seconds": 10}));
    rig.cluster.push_script(Script::hanging());
    let error = rig.env.write_seed(POD, &seed()).await.expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::TimedOut { .. })));
    assert!(rig.cluster.patches().is_empty());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_a_failed_hash_patch_is_a_fault() {
    let rig = rig();
    rig.cluster.fail_patches();
    rig.cluster.push_script(Script::like_bsdtar_extract());
    let error = rig.env.write_seed(POD, &seed()).await.expect_err("a fault");
    assert!(matches!(error, Error::Cluster(_)));
    assert!(!PortError::from(error).is_refusal());
}
