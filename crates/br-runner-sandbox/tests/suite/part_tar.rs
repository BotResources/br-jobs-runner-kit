use br_runner_sandbox::{TarCounter, TarError};
use tar::{Builder, EntryType, Header};

fn gnu_header(kind: EntryType, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(0o644);
    header.set_mtime(0);
    header
}

pub fn sample_tar() -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    let mut dir = gnu_header(EntryType::Directory, 0);
    builder
        .append_data(&mut dir, "work/", std::io::empty())
        .expect("dir");
    let mut file = gnu_header(EntryType::Regular, 5);
    builder
        .append_data(&mut file, "work/a.txt", &b"hello"[..])
        .expect("file");
    let mut big = gnu_header(EntryType::Regular, 1500);
    builder
        .append_data(&mut big, "work/big.bin", &[7u8; 1500][..])
        .expect("big");
    let mut link = gnu_header(EntryType::Symlink, 0);
    builder
        .append_link(&mut link, "work/link", "a.txt")
        .expect("link");
    let mut hard = gnu_header(EntryType::Link, 0);
    builder
        .append_link(&mut hard, "work/hard", "work/a.txt")
        .expect("hard");
    builder.into_inner().expect("tar")
}

fn count(bytes: &[u8]) -> Result<u64, TarError> {
    let mut counter = TarCounter::new();
    counter.push(bytes)?;
    counter.finish()
}

#[test]
fn the_entries_of_a_tar_are_counted() {
    assert_eq!(count(&sample_tar()), Ok(5));
}

#[test]
fn the_count_does_not_depend_on_how_the_bytes_are_cut() {
    let tar = sample_tar();
    for step in [1, 7, 511, 512, 513, 4096] {
        let mut counter = TarCounter::new();
        for chunk in tar.chunks(step) {
            counter.push(chunk).expect("push");
        }
        assert_eq!(counter.entries(), 5, "step {step}");
        assert_eq!(counter.finish(), Ok(5), "step {step}");
    }
}

#[test]
fn an_empty_archive_has_no_entry() {
    let tar = Builder::new(Vec::new()).into_inner().expect("tar");
    assert_eq!(count(&tar), Ok(0));
    assert_eq!(count(&[]), Ok(0));
}

#[test]
fn a_long_name_extension_is_not_an_entry() {
    let name = format!("work/{}", "d/".repeat(120));
    let mut builder = Builder::new(Vec::new());
    let mut header = gnu_header(EntryType::Regular, 3);
    builder
        .append_data(&mut header, format!("{name}file"), &b"abc"[..])
        .expect("file");
    assert_eq!(count(&builder.into_inner().expect("tar")), Ok(1));
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let body = format!(" {key}={value}\n");
    let mut length = body.len() + 1;
    while format!("{length}{body}").len() != length {
        length = format!("{length}{body}").len();
    }
    format!("{length}{body}").into_bytes()
}

fn pax_archive(data: &[u8]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    let records = pax_record("path", "work/pax.txt");
    let mut meta = gnu_header(EntryType::XHeader, records.len() as u64);
    builder
        .append_data(&mut meta, "PaxHeader/pax.txt", records.as_slice())
        .expect("meta");
    let mut file = gnu_header(EntryType::Regular, data.len() as u64);
    builder
        .append_data(&mut file, "work/short", data)
        .expect("file");
    builder.into_inner().expect("tar")
}

#[test]
fn a_pax_header_is_not_an_entry() {
    assert_eq!(count(&pax_archive(b"abc")), Ok(1));
}

#[test]
fn the_size_of_a_pax_header_is_the_size_of_the_data_that_follows() {
    let data = vec![9u8; 1200];
    let mut builder = Builder::new(Vec::new());
    let records = pax_record("size", "1200");
    let mut meta = gnu_header(EntryType::XHeader, records.len() as u64);
    builder
        .append_data(&mut meta, "PaxHeader/x", records.as_slice())
        .expect("meta");
    let mut header = gnu_header(EntryType::Regular, 0);
    builder
        .append_data(&mut header, "work/large", std::io::empty())
        .expect("file");
    let mut tar = builder.into_inner().expect("tar");
    let end = tar.len() - 1024;
    tar.truncate(end);
    let mut padded = data.clone();
    padded.resize(1536, 0);
    tar.extend(padded);
    let mut second = gnu_header(EntryType::Regular, 1);
    let mut after = Builder::new(Vec::new());
    after
        .append_data(&mut second, "work/after", &b"x"[..])
        .expect("after");
    tar.extend(after.into_inner().expect("tar"));
    assert_eq!(count(&tar), Ok(2));
}

#[test]
fn a_tar_that_ends_inside_an_entry_is_refused() {
    let tar = sample_tar();
    for cut in [100, 512 + 100, 1024 + 200] {
        assert_eq!(count(&tar[..cut]), Err(TarError::Truncated), "cut {cut}");
    }
}

#[test]
fn a_header_with_a_wrong_checksum_is_refused() {
    let mut tar = sample_tar();
    tar[0] ^= 0xff;
    assert_eq!(count(&tar), Err(TarError::Checksum));
}

#[test]
fn bytes_that_are_not_a_tar_are_refused() {
    assert!(count(&[b'x'; 512]).is_err());
}
