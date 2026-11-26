use std::sync::Arc;

use bytes::Bytes;

use super::{
    Bucket, BucketError, BucketFuture, ObjectInfo, RECORDS_PREFIX, SAVES_PREFIX, TOMBSTONES_PREFIX,
};

pub struct GuardedBucket {
    inner: Arc<dyn Bucket>,
}

fn readable_or_deletable(key: &str) -> bool {
    [SAVES_PREFIX, RECORDS_PREFIX, TOMBSTONES_PREFIX]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

fn forbidden<T>(action: &'static str, key: &str) -> BucketFuture<'static, T> {
    let error = BucketError::Forbidden {
        action,
        key: key.to_owned(),
    };
    Box::pin(async move { Err(error) })
}

impl GuardedBucket {
    pub fn new(inner: Arc<dyn Bucket>) -> Self {
        Self { inner }
    }
}

impl Bucket for GuardedBucket {
    fn check(&self) -> BucketFuture<'_, ()> {
        self.inner.check()
    }

    fn list_page<'a>(
        &'a self,
        prefix: &'a str,
        after: Option<&'a str>,
        limit: usize,
    ) -> BucketFuture<'a, Vec<ObjectInfo>> {
        if !readable_or_deletable(prefix) {
            return forbidden("list", prefix);
        }
        self.inner.list_page(prefix, after, limit)
    }

    fn get<'a>(&'a self, key: &'a str) -> BucketFuture<'a, Option<Bytes>> {
        if !readable_or_deletable(key) {
            return forbidden("read", key);
        }
        self.inner.get(key)
    }

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> BucketFuture<'a, ()> {
        if !key.starts_with(TOMBSTONES_PREFIX) {
            return forbidden("write", key);
        }
        self.inner.put(key, body)
    }

    fn delete<'a>(&'a self, key: &'a str) -> BucketFuture<'a, ()> {
        if !readable_or_deletable(key) {
            return forbidden("delete", key);
        }
        self.inner.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::FakeBucket;

    fn guarded() -> (Arc<FakeBucket>, GuardedBucket) {
        let fake = Arc::new(FakeBucket::default());
        (fake.clone(), GuardedBucket::new(fake))
    }

    #[tokio::test]
    async fn a_write_goes_through_under_tombstones_only() {
        let (fake, bucket) = guarded();
        bucket.put("tombstones/c/t/a/u.json", Bytes::from_static(b"x")).await.unwrap();
        assert!(fake.contains("tombstones/c/t/a/u.json"));
        for key in ["saves/c/t/a/u/manifest.json", "records/c/t/a/u.json", "other/x", "tombstones"] {
            let result = bucket.put(key, Bytes::from_static(b"x")).await;
            assert_eq!(
                result,
                Err(BucketError::Forbidden { action: "write", key: key.to_owned() }),
                "{key}"
            );
            assert!(!fake.contains(key));
        }
    }

    #[tokio::test]
    async fn a_delete_a_read_and_a_list_go_through_under_the_three_prefixes_only() {
        let (fake, bucket) = guarded();
        for key in ["saves/a", "records/a", "tombstones/a", "other/a"] {
            fake.insert(key, b"x", 0);
        }
        for key in ["saves/a", "records/a", "tombstones/a"] {
            assert!(bucket.get(key).await.unwrap().is_some());
            bucket.delete(key).await.unwrap();
            assert!(!fake.contains(key));
        }
        assert!(matches!(bucket.get("other/a").await, Err(BucketError::Forbidden { .. })));
        assert!(matches!(bucket.delete("other/a").await, Err(BucketError::Forbidden { .. })));
        assert!(fake.contains("other/a"));
        assert!(matches!(
            bucket.list_page("other/", None, 10).await,
            Err(BucketError::Forbidden { .. })
        ));
        assert!(bucket.list_page("saves/", None, 10).await.is_ok());
    }
}
