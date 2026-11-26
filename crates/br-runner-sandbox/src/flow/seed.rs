use br_agentic_harness_contract::Seed;
use bytes::Bytes;

use crate::decide::seed_hash_patch;
use crate::env::Env;
use crate::error::Error;
use crate::exec::{argv, bounded, succeed};
use crate::seed::{seed_archive, seed_hash_text};

const OPERATION: &str = "seed";

impl Env {
    pub async fn write_seed(&self, pod: &str, seed: &Seed) -> Result<(), Error> {
        if let Some(archive) = seed_archive(seed)? {
            let command = argv::tar_seed(self.config.seed_tools_path(), self.config.seed_root());
            let io = self.cluster.exec(self.seed_request(pod, command)).await?;
            bounded::<_, Error>(OPERATION, self.config.exec_timeout(), async {
                succeed(OPERATION, io, Some(Bytes::from(archive))).await?;
                Ok(())
            })
            .await?;
        }
        let patch = seed_hash_patch(&self.pod_keys(), &seed_hash_text(seed));
        self.cluster.patch_pod(pod, &patch).await?;
        Ok(())
    }
}
