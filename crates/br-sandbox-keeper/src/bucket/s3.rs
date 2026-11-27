use bytes::Bytes;
use futures::StreamExt;
use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::path::Path;
use object_store::{Error as BackendError, ObjectStore, ObjectStoreExt, PutPayload};

use br_runner_sandbox::Timestamp;

use crate::config::StoreSettings;

use super::{Bucket, BucketError, BucketFuture, ObjectInfo, SAVES_PREFIX};

pub struct S3Bucket {
    backend: AmazonS3,
}

impl std::fmt::Debug for S3Bucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("S3Bucket")
    }
}

fn call(error: BackendError) -> BucketError {
    BucketError::Call(error.to_string())
}

fn path(key: &str) -> Result<Path, BucketError> {
    Path::parse(key.trim_end_matches('/'))
        .map_err(|_| BucketError::Call(format!("{key:?} is not a valid key")))
}

fn missing_bucket(error: &BackendError) -> bool {
    error.to_string().contains("NoSuchBucket")
}

impl S3Bucket {
    pub fn new(
        settings: &StoreSettings,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<Self, BucketError> {
        crate::tls::ensure_crypto_provider();
        let backend = AmazonS3Builder::new()
            .with_endpoint(settings.endpoint.as_str())
            .with_bucket_name(&settings.bucket)
            .with_region(&settings.region)
            .with_access_key_id(access_key_id)
            .with_secret_access_key(secret_access_key)
            .with_virtual_hosted_style_request(!settings.path_style)
            .with_allow_http(settings.endpoint.scheme() == "http")
            .with_disable_bulk_delete(true)
            .build()
            .map_err(call)?;
        Ok(Self { backend })
    }
}

impl crate::sealed::Sealed for S3Bucket {}

impl Bucket for S3Bucket {
    fn check(&self) -> BucketFuture<'_, ()> {
        Box::pin(async move {
            let mut listing = self.backend.list(Some(&path(SAVES_PREFIX)?));
            match listing.next().await {
                None | Some(Ok(_)) => Ok(()),
                Some(Err(error)) if missing_bucket(&error) => Err(BucketError::Absent),
                Some(Err(error)) => Err(BucketError::Unreachable(error.to_string())),
            }
        })
    }

    fn list_page<'a>(
        &'a self,
        prefix: &'a str,
        after: Option<&'a str>,
        limit: usize,
    ) -> BucketFuture<'a, Vec<ObjectInfo>> {
        Box::pin(async move {
            let prefix = path(prefix)?;
            let stream = match after {
                Some(key) => self.backend.list_with_offset(Some(&prefix), &path(key)?),
                None => self.backend.list(Some(&prefix)),
            };
            let mut page = Vec::new();
            let mut stream = stream.take(limit);
            while let Some(meta) = stream.next().await {
                let meta = meta.map_err(call)?;
                page.push(ObjectInfo {
                    key: meta.location.to_string(),
                    last_modified: Timestamp::from_system_time(meta.last_modified.into()),
                });
            }
            Ok(page)
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> BucketFuture<'a, Option<Bytes>> {
        Box::pin(async move {
            match self.backend.get(&path(key)?).await {
                Ok(result) => result.bytes().await.map(Some).map_err(call),
                Err(BackendError::NotFound { .. }) => Ok(None),
                Err(error) => Err(call(error)),
            }
        })
    }

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> BucketFuture<'a, ()> {
        Box::pin(async move {
            self.backend
                .put(&path(key)?, PutPayload::from(body))
                .await
                .map(|_| ())
                .map_err(call)
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BucketFuture<'a, ()> {
        Box::pin(async move {
            match self.backend.delete(&path(key)?).await {
                Ok(()) | Err(BackendError::NotFound { .. }) => Ok(()),
                Err(error) => Err(call(error)),
            }
        })
    }
}

#[cfg(test)]
mod tests;
