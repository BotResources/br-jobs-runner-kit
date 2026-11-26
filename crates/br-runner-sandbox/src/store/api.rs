use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::BoxStream;

use super::error::StoreError;

pub type StoreFuture<'a, T> = BoxFuture<'a, Result<T, StoreError>>;
pub type PartStream = BoxStream<'static, Result<Bytes, StoreError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    Created,
    AlreadyExists,
}

pub trait PartUpload: Send {
    fn write(&mut self, chunk: Bytes) -> StoreFuture<'_, ()>;

    fn finish(self: Box<Self>) -> StoreFuture<'static, ()>;

    fn abort(self: Box<Self>) -> StoreFuture<'static, ()>;
}

pub trait Store: Send + Sync {
    fn get<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<Bytes>>;

    fn get_stream<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<PartStream>>;

    fn size<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<u64>>;

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> StoreFuture<'a, ()>;

    fn put_if_absent<'a>(&'a self, key: &'a str, body: Bytes) -> StoreFuture<'a, PutOutcome>;

    fn start_upload<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Box<dyn PartUpload>>;

    fn list<'a>(&'a self, prefix: &'a str) -> StoreFuture<'a, Vec<String>>;

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()>;
}
