use serde_json::{Value, json};

use crate::config::{KeeperConfig, PoolConfig};

pub fn document() -> Value {
    json!({
        "store_bucket": "saves",
        "store_endpoint": "http://store.test:8333",
        "store_region": "eu",
        "listen_address": "127.0.0.1:0",
        "save_ttl_seconds": 1000,
        "skew_margin_seconds": 60,
        "pools": [{
            "id": "p",
            "namespace": "sbx",
            "replicaset": "warm-p",
            "min_replicas": 1,
            "max_replicas": 20
        }]
    })
}

pub fn config_of(document: &Value) -> KeeperConfig {
    KeeperConfig::from_json(&document.to_string()).unwrap()
}

pub fn config() -> KeeperConfig {
    config_of(&document())
}

pub fn paris() -> jiff::tz::TimeZone {
    jiff::tz::TimeZone::get("Europe/Paris").unwrap()
}

pub fn pool() -> PoolConfig {
    config().pools.remove(0)
}

pub mod saves {
    use br_agentic_harness_contract::{AgentId, ThreadRef, TurnId};
    use br_runner_sandbox::{
        AttemptId, ClientId, Manifest, PartReport, SaveKey, Sha256Hex, Timestamp, manifest_key,
        part_key,
    };
    use uuid::Uuid;

    pub fn save_key() -> SaveKey {
        SaveKey::new(
            ClientId::new("acme").unwrap(),
            ThreadRef::from_uuid(Uuid::now_v7()).unwrap(),
            AgentId::from_uuid(Uuid::now_v7()).unwrap(),
            TurnId::from_uuid(Uuid::now_v7()).unwrap(),
        )
    }

    pub fn manifest(key: &SaveKey, attempt: AttemptId, made_at: Timestamp) -> Manifest {
        Manifest::new(
            key.clone(),
            None,
            attempt,
            made_at,
            vec!["/work".to_owned()],
            PartReport {
                entries: 1,
                bytes_raw: 10,
                bytes_stored: 5,
                sha256_raw: Sha256Hex::of(b"raw"),
                sha256_stored: Sha256Hex::of(b"stored"),
            },
        )
    }

    pub fn manifest_object(key: &SaveKey, manifest: &Manifest) -> (String, Vec<u8>) {
        (manifest_key(key), manifest.to_vec().unwrap())
    }

    pub fn part_object(key: &SaveKey, attempt: AttemptId) -> String {
        part_key(key, attempt)
    }
}
