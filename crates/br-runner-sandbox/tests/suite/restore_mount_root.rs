use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use br_runner_sandbox::{AbsolutePath, argv};

const OLD_TIME: u64 = 1_577_836_800;

struct Machine {
    bsdtar: AbsolutePath,
    uid: u32,
    gid: u32,
    root: String,
}

fn absolute(text: &str) -> AbsolutePath {
    AbsolutePath::new("test", text).expect("a path")
}

fn bsdtar_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|folder| folder.join("bsdtar"))
        .find(|candidate| candidate.is_file())
}

fn own_ids() -> (u32, u32) {
    let probe = std::env::temp_dir().join(format!("br-sbx-ids-{}", std::process::id()));
    fs::create_dir_all(&probe).expect("a probe folder");
    let metadata = fs::metadata(&probe).expect("metadata");
    fs::remove_dir(&probe).expect("the probe folder goes");
    (metadata.uid(), metadata.gid())
}

fn foreign_root(uid: u32) -> Option<String> {
    ["/private/tmp", "/tmp"].into_iter().find_map(|candidate| {
        let metadata = fs::symlink_metadata(candidate).ok()?;
        (metadata.is_dir() && metadata.uid() != uid).then(|| candidate.to_owned())
    })
}

fn machine() -> Option<Machine> {
    let bsdtar = bsdtar_on_path()?;
    let (uid, gid) = own_ids();
    let root = foreign_root(uid)?;
    Some(Machine {
        bsdtar: absolute(bsdtar.to_str().expect("a utf-8 path")),
        uid,
        gid,
        root,
    })
}

fn entry(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &str,
    kind: tar::EntryType,
    mode: u32,
    owner: (u64, u64),
    content: &[u8],
) {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_uid(owner.0);
    header.set_gid(owner.1);
    header.set_mtime(OLD_TIME);
    header.set_size(content.len() as u64);
    builder
        .append_data(&mut header, path, content)
        .expect("an entry");
}

fn save_of(machine: &Machine, leaf: &str) -> Vec<u8> {
    let root = machine.root.trim_start_matches('/');
    let mut builder = tar::Builder::new(Vec::new());
    let own = (u64::from(machine.uid), u64::from(machine.gid));
    entry(
        &mut builder,
        root,
        tar::EntryType::Directory,
        0o777,
        (0, 0),
        b"",
    );
    entry(
        &mut builder,
        &format!("{root}/{leaf}"),
        tar::EntryType::Directory,
        0o755,
        own,
        b"",
    );
    entry(
        &mut builder,
        &format!("{root}/{leaf}/note"),
        tar::EntryType::Regular,
        0o644,
        own,
        b"kept",
    );
    builder.into_inner().expect("the archive")
}

fn run(command: &[String], stdin: &[u8]) -> bool {
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("bsdtar starts");
    child
        .stdin
        .take()
        .expect("a stdin")
        .write_all(stdin)
        .expect("the archive goes in");
    child.wait().expect("an exit").success()
}

fn leaf_folder(machine: &Machine, leaf: &str) -> PathBuf {
    Path::new(&machine.root).join(leaf)
}

#[test]
fn a_restore_over_a_scope_root_that_the_user_does_not_own_ends_well_and_keeps_the_content() {
    let Some(machine) = machine() else {
        eprintln!("skipped: no bsdtar on the path, or no folder here that the user does not own");
        return;
    };
    let leaf = format!("br-sbx-restore-{}", std::process::id());
    let save = save_of(&machine, &leaf);
    let folder = leaf_folder(&machine, &leaf);
    let scope = [absolute(&machine.root)];

    let plain = argv::tar_restore(&machine.bsdtar);
    assert!(
        !run(&plain, &save),
        "the unscoped call must fail on the foreign root"
    );
    fs::remove_dir_all(&folder).ok();

    let scoped = argv::tar_restore_scoped(&machine.bsdtar, &scope);
    let restored = run(&scoped, &save);
    let note = fs::read(folder.join("note"));
    let modified = fs::metadata(&folder).map(|metadata| metadata.mtime());
    fs::remove_dir_all(&folder).ok();

    assert!(restored, "the scoped call must end with exit 0");
    assert_eq!(note.expect("the file comes back"), b"kept");
    assert_eq!(modified.expect("the folder comes back"), OLD_TIME as i64);
}

#[test]
fn a_failed_extraction_is_still_a_failed_restore_with_the_scoped_call() {
    let Some(machine) = machine() else {
        eprintln!("skipped: no bsdtar on the path, or no folder here that the user does not own");
        return;
    };
    let scope = [absolute(&machine.root)];
    let scoped = argv::tar_restore_scoped(&machine.bsdtar, &scope);
    assert!(!run(&scoped, b"this is not an archive"));
}
