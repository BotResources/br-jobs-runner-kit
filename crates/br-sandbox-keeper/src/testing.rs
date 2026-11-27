use serde_json::{Value, json};

use crate::config::KeeperConfig;
use crate::pool::{PoolSettings, PoolSize, ResourceBounds};

pub fn document() -> Value {
    json!({
        "store_bucket": "saves",
        "store_endpoint": "http://store.test:8333",
        "store_region": "eu",
        "listen_address": "127.0.0.1:0",
        "save_ttl_seconds": 1000,
        "skew_margin_seconds": 60,
        "agent_container": "agent"
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

pub fn settings() -> PoolSettings {
    PoolSettings {
        image: "registry.example/agent:1".to_owned(),
        size: PoolSize::new(
            ResourceBounds::new("500m", "2"),
            ResourceBounds::new("1Gi", "4Gi"),
            ResourceBounds::new("1Gi", "8Gi"),
        ),
        min_replicas: 1,
        max_replicas: 20,
        refill: std::time::Duration::from_secs(3),
        margin: 2,
        floors: Vec::new(),
    }
}

pub mod saves {
    use br_agentic_harness_contract::{AgentId, ThreadRef, TurnId};
    use br_runner_sandbox::{
        AttemptId, ClientId, Manifest, PartReport, SaveKey, Sha256Hex, Timestamp, manifest_key,
        part_key,
    };
    use uuid::Uuid;

    pub const EARLY_TURN: u64 = 1_000;

    pub fn save_key() -> SaveKey {
        save_key_born(EARLY_TURN)
    }

    pub fn save_key_born(seconds: u64) -> SaveKey {
        let born = uuid::Timestamp::from_unix(uuid::NoContext, seconds, 0);
        SaveKey::new(
            ClientId::new("acme").unwrap(),
            ThreadRef::from_uuid(Uuid::now_v7()).unwrap(),
            AgentId::from_uuid(Uuid::now_v7()).unwrap(),
            TurnId::from_uuid(Uuid::new_v7(born)).unwrap(),
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

pub mod logs {
    use std::cell::RefCell;
    use std::io::Write;
    use std::sync::{Arc, Mutex, Once};

    #[derive(Clone, Default)]
    pub struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        pub fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    thread_local! {
        static SINK: RefCell<Option<Captured>> = const { RefCell::new(None) };
    }

    struct Routed;

    impl Write for Routed {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            SINK.with(|sink| match sink.borrow_mut().as_mut() {
                Some(captured) => captured.write(bytes),
                None => Ok(bytes.len()),
            })
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    static INSTALLED: Once = Once::new();

    pub struct CaptureGuard;

    impl Drop for CaptureGuard {
        fn drop(&mut self) {
            SINK.with(|sink| sink.borrow_mut().take());
        }
    }

    pub fn capture() -> (Captured, CaptureGuard) {
        INSTALLED.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_writer(|| Routed)
                .finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
        });
        let captured = Captured::default();
        SINK.with(|sink| *sink.borrow_mut() = Some(captured.clone()));
        (captured, CaptureGuard)
    }
}
