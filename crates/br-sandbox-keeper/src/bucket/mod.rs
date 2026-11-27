mod guard;
mod s3;

use br_runner_sandbox::Timestamp;
use bytes::Bytes;
use futures::future::BoxFuture;

pub use guard::GuardedBucket;
pub use s3::S3Bucket;

pub const SAVES_PREFIX: &str = "saves/";
pub const RECORDS_PREFIX: &str = "records/";
pub const TOMBSTONES_PREFIX: &str = "tombstones/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    pub key: String,
    pub last_modified: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BucketError {
    #[error("the bucket is absent")]
    Absent,
    #[error("the object store is not reachable: {0}")]
    Unreachable(String),
    #[error("the object store call failed: {0}")]
    Call(String),
    #[error("the keeper does not {action} the key {key:?}")]
    Forbidden { action: &'static str, key: String },
}

pub type BucketFuture<'a, T> = BoxFuture<'a, Result<T, BucketError>>;

pub trait Bucket: crate::sealed::Sealed + Send + Sync {
    fn check(&self) -> BucketFuture<'_, ()>;

    fn list_page<'a>(
        &'a self,
        prefix: &'a str,
        after: Option<&'a str>,
        limit: usize,
    ) -> BucketFuture<'a, Vec<ObjectInfo>>;

    fn get<'a>(&'a self, key: &'a str) -> BucketFuture<'a, Option<Bytes>>;

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> BucketFuture<'a, ()>;

    fn delete<'a>(&'a self, key: &'a str) -> BucketFuture<'a, ()>;
}
