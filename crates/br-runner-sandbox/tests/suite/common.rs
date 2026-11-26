use br_agentic_harness_contract::{AgentId, ThreadRef, TurnId};
use br_runner_sandbox::{ConfigError, SandboxConfig, SandboxConfigInput};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn required() -> Value {
    json!({
        "namespace": "sbx-acme-none",
        "client_id": "acme",
        "pool_id": "main",
        "network_mode": "none",
        "save_ttl_seconds": 3600,
        "store_endpoint": "https://store.example.test",
        "store_bucket": "saves",
        "store_region": "eu-west-1",
        "store_credentials": {"access_key_id": "AKIDEXAMPLE", "secret_access_key": "wJalrSECRETEXAMPLE"},
    })
}

pub fn with(extra: Value) -> Value {
    let mut base = required();
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        for (key, value) in extra {
            base.insert(key, value);
        }
    }
    base
}

pub fn without(key: &str) -> Value {
    let mut base = required();
    if let Some(map) = base.as_object_mut() {
        map.remove(key);
    }
    base
}

pub fn input(value: Value) -> Result<SandboxConfigInput, serde_json::Error> {
    serde_json::from_value(value)
}

pub fn build(value: Value) -> Result<SandboxConfig, ConfigError> {
    SandboxConfig::try_from(input(value).expect("the input deserializes"))
}

pub fn config() -> SandboxConfig {
    build(required()).expect("the required keys build a configuration")
}

pub fn thread() -> ThreadRef {
    ThreadRef::from_uuid(Uuid::now_v7()).expect("a v7 thread")
}

pub fn agent() -> AgentId {
    AgentId::from_uuid(Uuid::now_v7()).expect("an agent")
}

pub fn turn() -> TurnId {
    TurnId::from_uuid(Uuid::now_v7()).expect("a turn")
}
