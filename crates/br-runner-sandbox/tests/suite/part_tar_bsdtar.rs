use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_runner_sandbox::{TarCounter, TarError};

use crate::restore_mount_root::{bsdtar_on_path, skipped};

const END_BLOCKS: u64 = 1024;
const BLOCK: u64 = 512;

fn scratch_folder() -> PathBuf {
    static FOLDERS: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "br-sbx-tar-{}-{}",
        std::process::id(),
        FOLDERS.fetch_add(1, Ordering::SeqCst)
    ))
}

fn bsdtar_stream(bsdtar: &std::path::Path) -> Vec<u8> {
    let folder = scratch_folder();
    fs::create_dir_all(folder.join("sub")).expect("a scratch folder");
    fs::write(folder.join("a.txt"), b"hello").expect("a file");
    fs::write(folder.join("sub").join("b.bin"), vec![7u8; 2000]).expect("a file");
    fs::write(folder.join("sub").join("c.txt"), b"c").expect("a file");
    let output = Command::new(bsdtar)
        .arg("--create")
        .arg("--file")
        .arg("-")
        .arg("-C")
        .arg(&folder)
        .arg(".")
        .output()
        .expect("bsdtar runs");
    fs::remove_dir_all(&folder).expect("the scratch folder goes");
    assert!(output.status.success(), "bsdtar failed");
    output.stdout
}

fn count(bytes: &[u8]) -> Result<u64, TarError> {
    let mut counter = TarCounter::new();
    counter.push(bytes)?;
    counter.finish()
}

struct Layout {
    entries: u64,
    headers: Vec<u64>,
    end: u64,
}

fn layout(bytes: &[u8]) -> Layout {
    let mut archive = tar::Archive::new(bytes);
    let mut headers = Vec::new();
    let mut entries = 0;
    let mut end = 0;
    for entry in archive.entries().expect("entries") {
        let entry = entry.expect("an entry");
        headers.push(entry.raw_header_position());
        let size = entry.header().size().expect("a size");
        end = entry.raw_file_position() + size.div_ceil(BLOCK) * BLOCK;
        entries += 1;
    }
    Layout {
        entries,
        headers,
        end,
    }
}

#[test]
fn a_real_bsdtar_stream_is_counted_when_complete_and_refused_when_cut_before_its_end_blocks() {
    let test =
        "a_real_bsdtar_stream_is_counted_when_complete_and_refused_when_cut_before_its_end_blocks";
    let Some(bsdtar) = bsdtar_on_path() else {
        skipped::<()>(test, "no bsdtar on the path");
        return;
    };
    let stream = bsdtar_stream(&bsdtar);
    let layout = layout(&stream);
    assert!(layout.entries >= 4, "{} entries", layout.entries);
    assert_eq!(count(&stream), Ok(layout.entries));
    let mut cuts: Vec<u64> = layout
        .headers
        .iter()
        .copied()
        .filter(|cut| *cut > 0)
        .collect();
    cuts.extend([
        layout.end,
        layout.end + 1,
        layout.end + BLOCK,
        layout.end + END_BLOCKS - 1,
    ]);
    for cut in cuts {
        let cut = usize::try_from(cut).expect("a small offset");
        assert_eq!(count(&stream[..cut]), Err(TarError::Truncated), "cut {cut}");
    }
    let complete = usize::try_from(layout.end + END_BLOCKS).expect("a small offset");
    assert_eq!(count(&stream[..complete]), Ok(layout.entries));
}
