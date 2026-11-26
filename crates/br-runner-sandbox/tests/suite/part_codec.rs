use br_runner_sandbox::{
    FormatError, ManifestPart, PartDecoder, PartEncoder, PartReport, Sha256Hex,
};
use sha2::{Digest, Sha256};

use crate::part_tar::sample_tar;

fn encode(raw: &[u8], level: i32, step: usize) -> (Vec<u8>, PartReport) {
    let mut encoder = PartEncoder::new(level).expect("encoder");
    let mut stored = Vec::new();
    for chunk in raw.chunks(step.max(1)) {
        stored.extend(encoder.push(chunk).expect("push"));
    }
    let (tail, report) = encoder.finish().expect("finish");
    stored.extend(tail);
    (stored, report)
}

fn part_of(report: &PartReport) -> ManifestPart {
    ManifestPart {
        index: 0,
        key: "key".to_owned(),
        bytes_raw: report.bytes_raw,
        bytes_stored: report.bytes_stored,
        sha256_raw: report.sha256_raw,
        sha256_stored: report.sha256_stored,
    }
}

fn decode(part: &ManifestPart, stored: &[u8], step: usize) -> Result<Vec<u8>, FormatError> {
    let mut decoder = PartDecoder::new(part)?;
    let mut raw = Vec::new();
    for chunk in stored.chunks(step.max(1)) {
        raw.extend(decoder.push(chunk)?);
    }
    raw.extend(decoder.finish()?);
    Ok(raw)
}

fn big_tar() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for index in 0..20 {
        let mut header = tar::Header::new_gnu();
        let bytes: Vec<u8> = (0..30_000u32)
            .map(|n| ((n * 31 + index) % 251) as u8)
            .collect();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        builder
            .append_data(&mut header, format!("work/f{index}"), bytes.as_slice())
            .expect("file");
    }
    builder.into_inner().expect("tar")
}

#[test]
fn a_part_is_one_zstd_stream_of_the_tar_that_decodes_to_the_tar() {
    let tar = big_tar();
    let (stored, report) = encode(&tar, 3, 4096);
    assert_eq!(zstd::decode_all(stored.as_slice()).expect("zstd"), tar);
    assert_eq!(decode(&part_of(&report), &stored, 1000), Ok(tar));
}

#[test]
fn the_report_holds_the_sizes_the_hashes_and_the_entry_count() {
    let tar = sample_tar();
    let (stored, report) = encode(&tar, 3, 300);
    assert_eq!(report.entries, 5);
    assert_eq!(report.bytes_raw, tar.len() as u64);
    assert_eq!(report.bytes_stored, stored.len() as u64);
    assert_eq!(
        report.sha256_raw.as_bytes(),
        &<[u8; 32]>::from(Sha256::digest(&tar))
    );
    assert_eq!(
        report.sha256_stored.as_bytes(),
        &<[u8; 32]>::from(Sha256::digest(&stored))
    );
}

#[test]
fn the_part_is_smaller_than_a_compressible_tar() {
    let tar = big_tar();
    let (stored, _) = encode(&tar, 3, 8192);
    assert!(stored.len() < tar.len());
}

#[test]
fn every_level_that_zstd_accepts_gives_a_part_that_decodes() {
    let tar = sample_tar();
    for level in [1, 3, 19, 22] {
        let (stored, report) = encode(&tar, level, 512);
        assert_eq!(
            decode(&part_of(&report), &stored, 64),
            Ok(tar.clone()),
            "level {level}"
        );
    }
}

#[test]
fn a_level_that_zstd_does_not_accept_is_refused() {
    for level in [0, 23, -5] {
        assert!(
            matches!(PartEncoder::new(level), Err(FormatError::Level(found)) if found == level)
        );
    }
}

#[test]
fn the_decoder_gives_the_same_bytes_whatever_the_cut_of_the_stored_stream() {
    let tar = big_tar();
    let (stored, report) = encode(&tar, 3, 1000);
    for step in [1, 13, 4096, stored.len()] {
        assert_eq!(
            decode(&part_of(&report), &stored, step),
            Ok(tar.clone()),
            "step {step}"
        );
    }
}

#[test]
fn a_stored_stream_that_is_cut_short_is_refused() {
    let (stored, report) = encode(&big_tar(), 3, 4096);
    let result = decode(&part_of(&report), &stored[..stored.len() - 5], 4096);
    assert!(matches!(result, Err(FormatError::StoredSize { .. })));
}

#[test]
fn a_stored_stream_that_is_longer_than_the_manifest_says_is_refused_before_its_end() {
    let (mut stored, report) = encode(&big_tar(), 3, 4096);
    stored.extend_from_slice(&[0u8; 64]);
    let result = decode(&part_of(&report), &stored, 4096);
    assert!(matches!(result, Err(FormatError::StoredSize { .. })));
}

#[test]
fn a_flipped_byte_in_the_stored_stream_is_refused() {
    let (mut stored, report) = encode(&big_tar(), 3, 4096);
    let middle = stored.len() / 2;
    stored[middle] ^= 0x55;
    assert!(decode(&part_of(&report), &stored, 4096).is_err());
}

#[test]
fn a_wrong_stored_hash_is_refused() {
    let (stored, report) = encode(&sample_tar(), 3, 4096);
    let mut part = part_of(&report);
    part.sha256_stored = Sha256Hex::of(b"other");
    assert_eq!(
        decode(&part, &stored, 4096),
        Err(FormatError::Hash { which: "stored" })
    );
}

#[test]
fn a_wrong_raw_hash_is_refused() {
    let (stored, report) = encode(&sample_tar(), 3, 4096);
    let mut part = part_of(&report);
    part.sha256_raw = Sha256Hex::of(b"other");
    assert_eq!(
        decode(&part, &stored, 4096),
        Err(FormatError::Hash { which: "raw" })
    );
}

#[test]
fn a_wrong_raw_size_is_refused() {
    let (stored, report) = encode(&big_tar(), 3, 4096);
    let mut part = part_of(&report);
    part.bytes_raw -= 1;
    assert!(matches!(
        decode(&part, &stored, 4096),
        Err(FormatError::RawSize { .. })
    ));
    let mut part = part_of(&report);
    part.bytes_raw += 1;
    assert!(matches!(
        decode(&part, &stored, 4096),
        Err(FormatError::RawLength { .. })
    ));
}

#[test]
fn a_stream_that_expands_beyond_the_manifest_is_stopped_at_the_limit() {
    let bomb = vec![0u8; 8 * 1024 * 1024];
    let stored = zstd::encode_all(bomb.as_slice(), 3).expect("zstd");
    let part = ManifestPart {
        index: 0,
        key: "key".to_owned(),
        bytes_raw: 1024,
        bytes_stored: stored.len() as u64,
        sha256_raw: Sha256Hex::of(&bomb),
        sha256_stored: Sha256Hex::of(&stored),
    };
    assert!(matches!(
        decode(&part, &stored, stored.len()),
        Err(FormatError::RawSize { limit: 1024 })
    ));
}

#[test]
fn bytes_that_are_not_zstd_are_refused() {
    let junk = vec![0x41u8; 4096];
    let part = ManifestPart {
        index: 0,
        key: "key".to_owned(),
        bytes_raw: 4096,
        bytes_stored: 4096,
        sha256_raw: Sha256Hex::of(&junk),
        sha256_stored: Sha256Hex::of(&junk),
    };
    assert!(matches!(
        decode(&part, &junk, 4096),
        Err(FormatError::Compression(_))
    ));
}

#[test]
fn a_tar_that_is_cut_short_is_not_written_as_a_part() {
    let tar = sample_tar();
    let mut encoder = PartEncoder::new(3).expect("encoder");
    encoder.push(&tar[..700]).expect("push");
    assert!(matches!(encoder.finish(), Err(FormatError::Compression(_))));
}

#[test]
fn bytes_that_are_not_a_tar_are_not_written_as_a_part() {
    let mut encoder = PartEncoder::new(3).expect("encoder");
    assert!(encoder.push(&[b'z'; 512]).is_err());
}

#[test]
fn a_digest_is_sixty_four_lowercase_hexadecimal_characters() {
    let digest = Sha256Hex::of(b"abc");
    assert_eq!(
        digest.to_string(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(Sha256Hex::parse(&digest.to_string()), Ok(digest));
    assert!(Sha256Hex::parse(&digest.to_string().to_uppercase()).is_err());
    assert!(Sha256Hex::parse("abc").is_err());
    assert!(Sha256Hex::parse(&"g".repeat(64)).is_err());
}
