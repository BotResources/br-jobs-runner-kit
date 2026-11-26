use bytes::Bytes;
use object_store::{MultipartUpload, WriteMultipart};

use super::api::{PartUpload, StoreFuture};
use super::s3::call;

const IN_FLIGHT_PARTS: usize = 4;

pub(super) struct S3Upload {
    inner: WriteMultipart,
}

impl S3Upload {
    pub(super) fn new(upload: Box<dyn MultipartUpload>) -> Self {
        Self {
            inner: WriteMultipart::new(upload),
        }
    }
}

impl PartUpload for S3Upload {
    fn write(&mut self, chunk: Bytes) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            self.inner
                .wait_for_capacity(IN_FLIGHT_PARTS)
                .await
                .map_err(call)?;
            self.inner.put(chunk);
            Ok(())
        })
    }

    fn finish(self: Box<Self>) -> StoreFuture<'static, ()> {
        Box::pin(async move { self.inner.finish().await.map(|_| ()).map_err(call) })
    }

    fn abort(self: Box<Self>) -> StoreFuture<'static, ()> {
        Box::pin(async move { self.inner.abort().await.map_err(call) })
    }
}
