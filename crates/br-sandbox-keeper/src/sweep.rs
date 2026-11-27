use std::sync::Arc;
use std::time::Duration;

use br_runner_sandbox::{Clock, Timestamp};
use bytes::Bytes;
use tokio_util::sync::CancellationToken;

use crate::bucket::{
    Bucket, BucketError, ObjectInfo, RECORDS_PREFIX, SAVES_PREFIX, TOMBSTONES_PREFIX,
};
use crate::config::KeeperConfig;
use crate::leader::Leadership;
use crate::metrics::Metrics;
use crate::rules::{
    ManifestRead, StoreStep, SweepRules, TurnGrouper, TurnListing, needs_manifest, needs_read,
    plan_turn, read_manifest, record_is_old, tombstone_is_old,
};

impl From<&KeeperConfig> for SweepRules {
    fn from(config: &KeeperConfig) -> Self {
        Self {
            save_ttl: config.save_ttl,
            skew_margin: config.skew_margin,
            attempt_ttl: config.attempt_ttl,
            tombstone_ttl: config.tombstone_ttl,
        }
    }
}

pub struct Sweeper {
    pub bucket: Arc<dyn Bucket>,
    pub clock: Arc<dyn Clock>,
    pub leadership: Arc<Leadership>,
    pub metrics: Arc<Metrics>,
    pub rules: SweepRules,
    pub page_size: usize,
    pub interval: Duration,
}

enum Stop {
    NotLeader,
    Failed,
}

impl Sweeper {
    pub async fn run(self, shutdown: CancellationToken) {
        loop {
            if self.leadership.is_leader() {
                self.sweep_once().await;
            }
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = tokio::time::sleep(self.interval) => {}
            }
        }
    }

    pub async fn sweep_once(&self) {
        let now = self.clock.now();
        for result in [
            self.sweep_saves(now).await,
            self.sweep_aged(RECORDS_PREFIX, now, record_is_old).await,
            self.sweep_aged(TOMBSTONES_PREFIX, now, tombstone_is_old)
                .await,
        ] {
            match result {
                Ok(()) | Err(Stop::NotLeader) => {}
                Err(Stop::Failed) => self.metrics.sweep_error(),
            }
        }
    }

    fn failed(&self, what: &str, key: &str, error: &BucketError) -> Stop {
        tracing::warn!(%error, key, "{what}");
        self.metrics.sweep_error();
        Stop::Failed
    }

    fn require_leader(&self) -> Result<(), Stop> {
        if self.leadership.is_leader() {
            Ok(())
        } else {
            Err(Stop::NotLeader)
        }
    }

    async fn next_page(
        &self,
        prefix: &str,
        after: &mut Option<String>,
    ) -> Result<Option<Vec<ObjectInfo>>, Stop> {
        self.require_leader()?;
        let page = self
            .bucket
            .list_page(prefix, after.as_deref(), self.page_size)
            .await
            .map_err(|error| {
                tracing::warn!(%error, prefix, "the listing failed");
                Stop::Failed
            })?;
        let Some(last) = page.last() else {
            return Ok(None);
        };
        let full = page.len() >= self.page_size;
        *after = full.then(|| last.key.clone());
        Ok(Some(page))
    }

    async fn sweep_saves(&self, now: Timestamp) -> Result<(), Stop> {
        let mut grouper = TurnGrouper::default();
        let mut after: Option<String> = None;
        loop {
            let Some(page) = self.next_page(SAVES_PREFIX, &mut after).await? else {
                break;
            };
            for info in page {
                if let Some(turn) = grouper.push(info) {
                    self.sweep_turn(&turn, now).await?;
                }
            }
            if after.is_none() {
                break;
            }
        }
        match grouper.finish() {
            Some(turn) => self.sweep_turn(&turn, now).await,
            None => Ok(()),
        }
    }

    async fn read_of(&self, turn: &TurnListing, now: Timestamp) -> Result<ManifestRead, Stop> {
        let Some(listed) = &turn.manifest else {
            return Ok(ManifestRead::Absent);
        };
        if !needs_manifest(turn, now, &self.rules) {
            return Ok(ManifestRead::NotNeeded);
        }
        match self.bucket.get(&listed.key).await {
            Ok(Some(bytes)) => Ok(read_manifest(turn, &bytes)),
            Ok(None) => Ok(ManifestRead::Absent),
            Err(error) => Err(self.failed("the manifest could not be read", &listed.key, &error)),
        }
    }

    async fn sweep_turn(&self, turn: &TurnListing, now: Timestamp) -> Result<(), Stop> {
        let read = match self.read_of(turn, now).await {
            Ok(read) => read,
            Err(Stop::Failed) => return Ok(()),
            Err(stop) => return Err(stop),
        };
        if read == ManifestRead::Unreadable {
            tracing::warn!(
                key = turn.manifest.as_ref().map(|listed| listed.key.as_str()),
                "a manifest cannot be read and is left in place"
            );
            self.metrics.sweep_error();
        }
        for step in plan_turn(turn, &read, now, &self.rules) {
            self.require_leader()?;
            if self.run_step(step).await.is_err() {
                return Ok(());
            }
        }
        Ok(())
    }

    async fn run_step(&self, step: StoreStep) -> Result<(), Stop> {
        match step {
            StoreStep::PutTombstone { key, body } => {
                self.bucket
                    .put(&key, Bytes::from(body))
                    .await
                    .map_err(|error| {
                        self.failed("a tombstone could not be written", &key, &error)
                    })?;
                self.metrics.tombstone_written();
            }
            StoreStep::DeleteManifest { key } => {
                self.bucket.delete(&key).await.map_err(|error| {
                    self.failed("a manifest could not be deleted", &key, &error)
                })?;
                self.metrics.save_deleted();
            }
            StoreStep::DeleteExpiredPart { key } => {
                self.bucket
                    .delete(&key)
                    .await
                    .map_err(|error| self.failed("a part could not be deleted", &key, &error))?;
            }
            StoreStep::DeleteOrphanAttempt { keys } => {
                for key in keys {
                    self.bucket.delete(&key).await.map_err(|error| {
                        self.failed("an orphan part could not be deleted", &key, &error)
                    })?;
                }
                self.metrics.orphan_deleted();
            }
        }
        Ok(())
    }

    async fn sweep_aged(
        &self,
        prefix: &str,
        now: Timestamp,
        is_old: fn(&[u8], Timestamp, &SweepRules) -> Result<bool, br_runner_sandbox::FormatError>,
    ) -> Result<(), Stop> {
        let mut after: Option<String> = None;
        loop {
            let Some(page) = self.next_page(prefix, &mut after).await? else {
                return Ok(());
            };
            for info in page {
                self.sweep_aged_object(&info, now, is_old).await?;
            }
            if after.is_none() {
                return Ok(());
            }
        }
    }

    async fn sweep_aged_object(
        &self,
        info: &ObjectInfo,
        now: Timestamp,
        is_old: fn(&[u8], Timestamp, &SweepRules) -> Result<bool, br_runner_sandbox::FormatError>,
    ) -> Result<(), Stop> {
        if !needs_read(info, now, &self.rules) {
            return Ok(());
        }
        let bytes = match self.bucket.get(&info.key).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(()),
            Err(error) => {
                self.failed("an object could not be read", &info.key, &error);
                return Ok(());
            }
        };
        match is_old(&bytes, now, &self.rules) {
            Ok(true) => {
                self.require_leader()?;
                if let Err(error) = self.bucket.delete(&info.key).await {
                    self.failed("an old object could not be deleted", &info.key, &error);
                }
            }
            Ok(false) => {}
            Err(_) => {
                tracing::warn!(
                    key = info.key,
                    "an object cannot be read and is left in place"
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
