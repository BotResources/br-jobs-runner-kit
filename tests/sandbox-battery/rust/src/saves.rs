use br_agentic_harness_contract::{AgentId, ThreadRef, TurnId};
use br_runner_sandbox::{
    ClientId, Manifest, S3Store, SaveKey, Store, manifest_key, part_key, records_prefix,
    saves_prefix, tombstones_prefix,
};

use crate::adapter::Tunables;
use crate::env::Env;
use crate::error::{Res, data};

pub struct Saves {
    pub store: S3Store,
    pub client: ClientId,
}

pub struct ThreadObjects {
    pub saves: Vec<String>,
    pub records: Vec<String>,
    pub tombstones: Vec<String>,
}

impl ThreadObjects {
    pub fn total(&self) -> usize {
        self.saves.len() + self.records.len() + self.tombstones.len()
    }
}

impl Saves {
    pub fn of(env: &Env) -> Res<Self> {
        Self::through(env, &Tunables::standard(env))
    }

    pub fn through(env: &Env, tunables: &Tunables) -> Res<Self> {
        let config = tunables.config(env)?;
        Ok(Self {
            store: S3Store::new(config.store())?,
            client: config.client_id().clone(),
        })
    }

    pub fn key(&self, thread: ThreadRef, agent: AgentId, turn: TurnId) -> SaveKey {
        SaveKey::new(self.client.clone(), thread, agent, turn)
    }

    pub async fn manifest(&self, key: &SaveKey) -> Res<Option<Manifest>> {
        match self.store.get(&manifest_key(key)).await? {
            Some(bytes) => Manifest::parse_for(&bytes, key)
                .map(Some)
                .map_err(|error| data("manifest", error.to_string())),
            None => Ok(None),
        }
    }

    pub async fn part_of(&self, key: &SaveKey) -> Res<String> {
        let manifest = self
            .manifest(key)
            .await?
            .ok_or_else(|| data("manifest", "there is no manifest"))?;
        Ok(part_key(key, manifest.attempt()))
    }

    pub async fn stored_bytes(&self, key: &SaveKey) -> Res<u64> {
        let part = self.part_of(key).await?;
        Ok(self.store.size(&part).await?.unwrap_or(0))
    }

    pub async fn delete_manifest_and_part(&self, key: &SaveKey) -> Res {
        let part = self.part_of(key).await?;
        self.store.delete(&manifest_key(key)).await?;
        self.store.delete(&part).await?;
        Ok(())
    }

    pub async fn objects(&self, thread: ThreadRef) -> Res<ThreadObjects> {
        Ok(ThreadObjects {
            saves: self.store.list(&saves_prefix(&self.client, thread)).await?,
            records: self
                .store
                .list(&records_prefix(&self.client, thread))
                .await?,
            tombstones: self
                .store
                .list(&tombstones_prefix(&self.client, thread))
                .await?,
        })
    }
}
