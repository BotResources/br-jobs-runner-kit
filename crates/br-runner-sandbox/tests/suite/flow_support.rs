use br_runner_sandbox::{
    AttemptId, Manifest, PartEncoder, PartReport, Record, RecordError, RecordState, SaveKey,
    manifest_key, record_key,
};

use crate::fakes::{Ids, Rig, start_time};
use crate::part_tar::sample_tar;

pub fn record_of(rig: &Rig, key: &SaveKey) -> Record {
    let bytes = rig.store.object(&record_key(key)).expect("a record");
    Record::parse(&bytes).expect("a valid record")
}

pub fn failed_with(record: &Record) -> RecordError {
    match record.state() {
        RecordState::Failed { error, .. } => error,
        other => panic!("not a failed record: {other:?}"),
    }
}

pub fn position(log: &[String], needle: &str) -> usize {
    log.iter()
        .position(|line| line.starts_with(needle))
        .unwrap_or_else(|| panic!("{needle} is not in {log:?}"))
}

pub fn noisy_tar() -> Vec<u8> {
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let data: Vec<u8> = (0..400_000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 24) as u8
        })
        .collect();
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    builder
        .append_data(&mut header, "work/noise.bin", data.as_slice())
        .expect("entry");
    builder.into_inner().expect("tar")
}

pub struct Staged {
    pub ids: Ids,
    pub manifest: Manifest,
    pub tar: Vec<u8>,
}

pub fn encode(tar: &[u8]) -> (Vec<u8>, PartReport) {
    let mut encoder = PartEncoder::new(3).expect("an encoder");
    let mut stored = encoder.push(tar).expect("pushed");
    let (tail, report) = encoder.finish().expect("finished");
    stored.extend(tail);
    (stored, report)
}

pub fn stage_with(rig: &Rig, tar: Vec<u8>, change: impl FnOnce(&mut PartReport)) -> Staged {
    let ids = Ids::new();
    let key = ids.parent_key(rig);
    let (stored, mut report) = encode(&tar);
    change(&mut report);
    let manifest = Manifest::new(
        key.clone(),
        None,
        AttemptId::mint(),
        start_time(),
        vec!["/work".to_owned(), "/home/agent".to_owned()],
        report,
    );
    rig.store.insert(&manifest.part().key, stored);
    rig.store
        .insert(&manifest_key(&key), manifest.to_vec().expect("bytes"));
    Staged { ids, manifest, tar }
}

pub fn stage(rig: &Rig) -> Staged {
    stage_with(rig, sample_tar(), |_| {})
}
