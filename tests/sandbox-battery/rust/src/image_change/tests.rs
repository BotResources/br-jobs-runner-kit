use std::sync::{Arc, Mutex};

use super::*;
use crate::pool_guard::{PoolCalls, merge_body};
use crate::restore::Registry;
use crate::restore::tests::SIGNAL_TESTS;

const FAILING: &str = "registry.invalid/none:1";

struct FakeKube {
    log: Arc<Mutex<Vec<String>>>,
}

impl PoolCalls for FakeKube {
    async fn spec(&self) -> Res<Value> {
        self.log.lock().unwrap().push("spec".to_owned());
        Ok(json!({"image": "img:1", "minReplicas": 1}))
    }

    async fn patch(&self, sets: &[(String, Value)]) -> Res {
        self.log
            .lock()
            .unwrap()
            .push(format!("patch {}", merge_body(sets)));
        Ok(())
    }

    fn reset(&self, snapshot: &Value) -> Vec<String> {
        self.log.lock().unwrap().push(format!("reset {snapshot}"));
        Vec::new()
    }
}

#[test]
fn below_0_4_0_the_image_is_the_chart_value_under_the_configured_key() {
    assert_eq!(
        image_sets(ChartLine::Values, "app.image", FAILING),
        [("app.image".to_owned(), json!(FAILING))]
    );
}

#[test]
fn from_0_4_0_on_the_image_is_the_spec_field_image_whatever_the_values_key() {
    assert_eq!(
        image_sets(ChartLine::PoolResource, "app.image", FAILING),
        [("image".to_owned(), json!(FAILING))]
    );
}

#[test]
fn the_line_of_a_chart_version_picks_the_key() {
    for (version, key) in [
        ("0.3.9", "values.key"),
        ("0.4.0", "image"),
        ("0.5.2", "image"),
    ] {
        let line = ChartLine::of(version).unwrap();
        assert_eq!(
            image_sets(line, "values.key", FAILING)[0].0,
            key,
            "{version}"
        );
    }
}

#[tokio::test]
async fn the_pool_image_is_patched_alone_and_the_whole_spec_comes_back_on_drop() {
    let _serial = SIGNAL_TESTS.lock().await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let registry = Registry::new(|_| {});
    let mut guard = PoolGuard::with(
        FakeKube {
            log: Arc::clone(&log),
        },
        std::env::temp_dir(),
        Arc::clone(&registry),
    );
    guard
        .set(&image_sets(ChartLine::PoolResource, "ignored", FAILING))
        .await
        .unwrap();
    drop(guard);
    assert_eq!(
        *log.lock().unwrap(),
        [
            "spec".to_owned(),
            format!("patch {{\"spec\":{{\"image\":\"{FAILING}\"}}}}"),
            "reset {\"image\":\"img:1\",\"minReplicas\":1}".to_owned()
        ]
    );
}
