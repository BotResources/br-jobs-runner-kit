use crate::decide::StoreView;
use crate::env::Env;
use crate::error::Error;
use crate::formats::{Manifest, Record, Tombstone};
use crate::ids::SaveKey;
use crate::keys::{manifest_key, record_key, tombstone_key};

impl Env {
    pub async fn load_manifest(&self, key: &SaveKey) -> Result<Option<Manifest>, Error> {
        match self.store.get(&manifest_key(key)).await? {
            Some(bytes) => Ok(Some(Manifest::parse_for(&bytes, key)?)),
            None => Ok(None),
        }
    }

    pub async fn read_store_view(&self, key: &SaveKey) -> StoreView {
        self.try_store_view(key)
            .await
            .unwrap_or(StoreView::Unavailable)
    }

    async fn try_store_view(&self, key: &SaveKey) -> Result<StoreView, Error> {
        if let Some(manifest) = self.load_manifest(key).await? {
            return Ok(StoreView::Manifest(Box::new(manifest)));
        }
        if let Some(bytes) = self.store.get(&tombstone_key(key)).await? {
            return Ok(StoreView::Tombstone(Tombstone::parse(&bytes)?));
        }
        if let Some(bytes) = self.store.get(&record_key(key)).await? {
            return Ok(StoreView::Record(Record::parse(&bytes)?));
        }
        Ok(StoreView::Nothing)
    }
}
