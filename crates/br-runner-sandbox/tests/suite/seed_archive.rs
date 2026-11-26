use std::io::Read;

use br_agentic_harness_contract::{FilePath, Seed, SeedEntry};
use br_runner_sandbox::{SeedError, check_seed_path, seed_archive, seed_hash_text, seed_mode};
use tar::{Archive, EntryType};

fn path(text: &str) -> FilePath {
    FilePath::new(text).expect("path")
}

fn seed() -> Seed {
    Seed::new(vec![
        SeedEntry::directory(path("review"), 0o755),
        SeedEntry::file(path("review/SKILL.md"), 0o644, b"# skill".to_vec()),
        SeedEntry::file(path("review/run.sh"), 0o755, b"#!/bin/sh\n".to_vec()),
        SeedEntry::symlink(path("review/latest"), 0o777, path("SKILL.md")),
    ])
    .expect("seed")
}

struct Read1 {
    name: String,
    kind: EntryType,
    mode: u32,
    uid: u64,
    gid: u64,
    link: Option<String>,
    data: Vec<u8>,
}

fn entries(bytes: &[u8]) -> Vec<Read1> {
    let mut archive = Archive::new(bytes);
    archive
        .entries()
        .expect("entries")
        .map(|entry| {
            let mut entry = entry.expect("entry");
            let mut data = Vec::new();
            entry.read_to_end(&mut data).expect("data");
            Read1 {
                name: entry
                    .path()
                    .expect("path")
                    .to_string_lossy()
                    .trim_end_matches('/')
                    .to_owned(),
                kind: entry.header().entry_type(),
                mode: entry.header().mode().expect("mode"),
                uid: entry.header().uid().expect("uid"),
                gid: entry.header().gid().expect("gid"),
                link: entry
                    .link_name()
                    .expect("link")
                    .map(|link| link.to_string_lossy().into_owned()),
                data,
            }
        })
        .collect()
}

#[test]
fn every_entry_of_the_archive_has_the_owner_zero_and_zero() {
    let bytes = seed_archive(&seed()).expect("archive").expect("not empty");
    let read = entries(&bytes);
    assert_eq!(read.len(), 4);
    assert!(read.iter().all(|entry| entry.uid == 0 && entry.gid == 0));
}

#[test]
fn every_entry_has_its_mode_with_the_three_write_bits_removed() {
    let bytes = seed_archive(&seed()).expect("archive").expect("not empty");
    let read = entries(&bytes);
    let mode_of = |name: &str| {
        read.iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.mode)
    };
    assert_eq!(mode_of("review"), Some(0o555));
    assert_eq!(mode_of("review/SKILL.md"), Some(0o444));
    assert_eq!(mode_of("review/run.sh"), Some(0o555));
    assert_eq!(mode_of("review/latest"), Some(0o555));
}

#[test]
fn the_mode_function_removes_the_write_bits_and_keeps_the_others() {
    assert_eq!(seed_mode(0o777), 0o555);
    assert_eq!(seed_mode(0o644), 0o444);
    assert_eq!(seed_mode(0o200), 0o000);
    assert_eq!(seed_mode(0o4755), 0o4555);
    assert_eq!(seed_mode(0o444), 0o444);
}

#[test]
fn the_kinds_the_paths_the_links_and_the_content_are_the_ones_of_the_seed() {
    let bytes = seed_archive(&seed()).expect("archive").expect("not empty");
    let read = entries(&bytes);
    let find = |name: &str| read.iter().find(|entry| entry.name == name).expect(name);
    assert_eq!(find("review").kind, EntryType::Directory);
    assert_eq!(find("review/SKILL.md").kind, EntryType::Regular);
    assert_eq!(find("review/SKILL.md").data, b"# skill");
    assert_eq!(find("review/run.sh").data, b"#!/bin/sh\n");
    assert_eq!(find("review/latest").kind, EntryType::Symlink);
    assert_eq!(find("review/latest").link.as_deref(), Some("SKILL.md"));
}

#[test]
fn every_path_of_the_archive_is_relative_to_the_seed_root() {
    let bytes = seed_archive(&seed()).expect("archive").expect("not empty");
    for entry in entries(&bytes) {
        assert!(!entry.name.starts_with('/'), "{}", entry.name);
        assert!(
            !entry
                .name
                .split('/')
                .any(|part| part == ".." || part.is_empty()),
            "{}",
            entry.name
        );
    }
}

#[test]
fn an_empty_seed_set_gives_no_archive_so_nothing_is_written() {
    assert_eq!(seed_archive(&Seed::empty()), Ok(None));
}

#[test]
fn the_archive_of_a_seed_is_the_same_bytes_each_time() {
    assert_eq!(seed_archive(&seed()), seed_archive(&seed()));
}

#[test]
fn a_long_path_is_archived() {
    let long = format!("{}/file", "deep".repeat(40));
    let seed = Seed::new(vec![SeedEntry::file(path(&long), 0o644, b"x".to_vec())]).expect("seed");
    let bytes = seed_archive(&seed).expect("archive").expect("not empty");
    assert_eq!(entries(&bytes)[0].name, long);
}

#[test]
fn the_hash_text_is_the_content_hash_of_the_seed_set_and_follows_its_content() {
    let first = seed();
    assert_eq!(seed_hash_text(&first), first.content_hash().to_hex());
    assert_eq!(seed_hash_text(&first).len(), 64);
    assert_eq!(seed_hash_text(&first), seed_hash_text(&seed()));
    let other = Seed::new(vec![SeedEntry::file(
        path("review/SKILL.md"),
        0o644,
        b"# other".to_vec(),
    )])
    .expect("seed");
    assert_ne!(seed_hash_text(&first), seed_hash_text(&other));
    assert_ne!(seed_hash_text(&Seed::empty()), seed_hash_text(&first));
}

#[test]
fn a_path_that_is_empty_absolute_or_holds_a_parent_component_is_refused() {
    for text in ["", "/etc/passwd", "../x", "a/../b", "a/..", "\0"] {
        assert_eq!(
            check_seed_path(text),
            Err(SeedError::Path {
                path: text.to_owned()
            }),
            "{text:?}"
        );
    }
    for text in ["a", "a/b", "a/.hidden", "a..b/c"] {
        assert_eq!(check_seed_path(text), Ok(text));
    }
}

#[test]
fn a_refused_seed_path_is_a_port_refusal_and_an_archive_failure_is_a_fault() {
    let refusal = SeedError::Path {
        path: "/x".to_owned(),
    };
    assert!(refusal.is_refusal());
    assert!(!SeedError::Archive("io".to_owned()).is_refusal());
}
