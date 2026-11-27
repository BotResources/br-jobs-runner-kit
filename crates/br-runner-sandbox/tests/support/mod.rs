use br_runner_sandbox::{SandboxConfig, SandboxConfigInput};
use serde_json::json;

pub fn settings() -> SandboxConfig {
    let input: SandboxConfigInput = serde_json::from_value(json!({
        "namespace": "sbx-acme-none",
        "client_id": "acme",
        "pool_id": "main",
        "network_mode": "none",
        "save_ttl_seconds": 3600,
        "store_endpoint": "https://store.example.test",
        "store_bucket": "saves",
        "store_region": "eu-west-1",
        "store_credentials": {"access_key_id": "AKIDEXAMPLE", "secret_access_key": "wJalrSECRETEXAMPLE"},
    }))
    .expect("the input deserializes");
    SandboxConfig::try_from(input).expect("a configuration")
}
