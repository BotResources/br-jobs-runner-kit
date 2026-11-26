use br_agentic_harness_contract::{PortError, ThreadRef};

use crate::cluster::LabelSelector;
use crate::error::Error;
use crate::keys::{records_prefix, saves_prefix, tombstones_prefix};

use super::state::Sandbox;

const MANIFEST_SUFFIX: &str = "/manifest.json";
const TOMBSTONES: &str = "tombstones/";
const RECORDS: &str = "records/";

fn deletion_rank(key: &str) -> u8 {
    if key.ends_with(MANIFEST_SUFFIX) {
        0
    } else if key.starts_with(TOMBSTONES) {
        1
    } else if key.starts_with(RECORDS) {
        2
    } else {
        3
    }
}

impl Sandbox {
    pub async fn erase(&self, thread: ThreadRef) -> Result<(), PortError> {
        self.erase_thread(thread).await.map_err(PortError::from)
    }

    async fn erase_thread(&self, thread: ThreadRef) -> Result<(), Error> {
        let env = self.env();
        let selector = LabelSelector::new().with(env.pod_keys().thread(), thread.to_string());
        for pod in env.cluster.list_pods(&selector).await? {
            self.remove_pod(&pod.name).await?;
        }
        let client = env.config.client_id();
        let mut keys = Vec::new();
        for prefix in [
            saves_prefix(client, thread),
            records_prefix(client, thread),
            tombstones_prefix(client, thread),
        ] {
            keys.extend(env.store.list(&prefix).await?);
        }
        keys.sort_by_key(|key| (deletion_rank(key), key.clone()));
        for key in keys {
            env.store.delete(&key).await?;
        }
        Ok(())
    }
}
