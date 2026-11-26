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

pub fn pool() -> PoolConfig {
    config().pools.remove(0)
}
