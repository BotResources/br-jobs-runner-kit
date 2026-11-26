use bytes::Bytes;
use futures::StreamExt;
use object_store::aws::{AmazonS3, AmazonS3Builder, S3ConditionalPut};
use object_store::path::Path;
use object_store::{
    Error as BackendError, ObjectStore as Backend, ObjectStoreExt, PutMode, PutPayload,
};

use crate::config::StoreSettings;

use super::api::{PartStream, PartUpload, PutOutcome, Store, StoreFuture};
use super::error::StoreError;
use super::s3_upload::S3Upload;

pub struct S3Store {
    backend: AmazonS3,
}

impl std::fmt::Debug for S3Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("S3Store")
    }
}

pub(super) fn call(error: BackendError) -> StoreError {
    match error {
        BackendError::NotImplemented { .. } => StoreError::ConditionalCreateUnsupported,
        other => StoreError::Call(other.to_string()),
    }
}

fn path(key: &str) -> Result<Path, StoreError> {
    Path::parse(key).map_err(|_| StoreError::Key(key.to_owned()))
}

impl S3Store {
    pub fn new(settings: &StoreSettings) -> Result<Self, StoreError> {
        crate::tls::ensure_crypto_provider();
        let credentials = settings.credentials();
        let backend = AmazonS3Builder::new()
            .with_endpoint(settings.endpoint().as_str())
            .with_bucket_name(settings.bucket())
            .with_region(settings.region())
            .with_access_key_id(credentials.access_key_id())
            .with_secret_access_key(credentials.secret_access_key())
            .with_virtual_hosted_style_request(!settings.path_style())
            .with_allow_http(settings.endpoint().scheme() == "http")
            .with_conditional_put(S3ConditionalPut::ETagMatch)
            .with_disable_bulk_delete(true)
            .build()
            .map_err(call)?;
        Ok(Self { backend })
    }
}

fn found<T>(result: Result<T, BackendError>) -> Result<Option<T>, StoreError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(BackendError::NotFound { .. }) => Ok(None),
        Err(other) => Err(call(other)),
    }
}

impl Store for S3Store {
    fn get<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<Bytes>> {
        Box::pin(async move {
            let path = path(key)?;
            match found(self.backend.get(&path).await)? {
                Some(result) => result.bytes().await.map(Some).map_err(call),
                None => Ok(None),
            }
        })
    }

    fn get_stream<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<PartStream>> {
        Box::pin(async move {
            let path = path(key)?;
            let stream: Option<PartStream> = found(self.backend.get(&path).await)?.map(|result| {
                result
                    .into_stream()
                    .map(|chunk| chunk.map_err(call))
                    .boxed()
            });
            Ok(stream)
        })
    }

    fn size<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<u64>> {
        Box::pin(async move {
            let path = path(key)?;
            Ok(found(self.backend.head(&path).await)?.map(|meta| meta.size))
        })
    }

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let path = path(key)?;
            self.backend
                .put(&path, PutPayload::from(body))
                .await
                .map(|_| ())
                .map_err(call)
        })
    }

    fn put_if_absent<'a>(&'a self, key: &'a str, body: Bytes) -> StoreFuture<'a, PutOutcome> {
        Box::pin(async move {
            let path = path(key)?;
            match self
                .backend
                .put_opts(&path, PutPayload::from(body), PutMode::Create.into())
                .await
            {
                Ok(_) => Ok(PutOutcome::Created),
                Err(BackendError::AlreadyExists { .. }) => Ok(PutOutcome::AlreadyExists),
                Err(other) => Err(call(other)),
            }
        })
    }

    fn start_upload<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Box<dyn PartUpload>> {
        Box::pin(async move {
            let path = path(key)?;
            let upload = self.backend.put_multipart(&path).await.map_err(call)?;
            Ok(Box::new(S3Upload::new(upload)) as Box<dyn PartUpload>)
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> StoreFuture<'a, Vec<String>> {
        Box::pin(async move {
            let path = path(prefix.trim_end_matches('/'))?;
            let mut keys = Vec::new();
            let mut stream = self.backend.list(Some(&path));
            while let Some(meta) = stream.next().await {
                keys.push(meta.map_err(call)?.location.to_string());
            }
            Ok(keys)
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let path = path(key)?;
            match self.backend.delete(&path).await {
                Ok(()) | Err(BackendError::NotFound { .. }) => Ok(()),
                Err(other) => Err(call(other)),
            }
        })
    }
}
