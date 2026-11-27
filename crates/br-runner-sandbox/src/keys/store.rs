use crate::ids::{AttemptId, ClientId, SaveKey};

pub fn part_key(key: &SaveKey, attempt: AttemptId) -> String {
    format!("{}/{attempt}/part-0.tar.zst", save_dir(key))
}

pub fn manifest_key(key: &SaveKey) -> String {
    format!("{}/manifest.json", save_dir(key))
}

pub fn record_key(key: &SaveKey) -> String {
    format!(
        "records/{}/{}/{}/{}.json",
        key.client(),
        key.thread(),
        key.agent(),
        key.turn()
    )
}

pub fn tombstone_key(key: &SaveKey) -> String {
    format!(
        "tombstones/{}/{}/{}/{}.json",
        key.client(),
        key.thread(),
        key.agent(),
        key.turn()
    )
}

pub fn saves_prefix(client: &ClientId, thread: br_agentic_harness_contract::ThreadRef) -> String {
    format!("saves/{client}/{thread}/")
}

pub fn records_prefix(client: &ClientId, thread: br_agentic_harness_contract::ThreadRef) -> String {
    format!("records/{client}/{thread}/")
}

pub fn tombstones_prefix(
    client: &ClientId,
    thread: br_agentic_harness_contract::ThreadRef,
) -> String {
    format!("tombstones/{client}/{thread}/")
}

fn save_dir(key: &SaveKey) -> String {
    format!(
        "saves/{}/{}/{}/{}",
        key.client(),
        key.thread(),
        key.agent(),
        key.turn()
    )
}
