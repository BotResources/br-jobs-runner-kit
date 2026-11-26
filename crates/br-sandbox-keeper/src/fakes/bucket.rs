use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use br_runner_sandbox::Timestamp;
use bytes::Bytes;

use crate::bucket::{Bucket, BucketError, BucketFuture, ObjectInfo};
use crate::time::at;

#[derive(Default)]
pub struct FakeBucket {
    objects: Mutex<BTreeMap<String, (Vec<u8>, Timestamp)>>,
    log: Mutex<Vec<String>>,
    failing_deletes: Mutex<HashSet<String>>,
    failing_gets: Mutex<HashSet<String>>,
    check_error: Mutex<Option<BucketError>>,
    list_calls: Mutex<Vec<(String, Option<String>, usize)>>,
}

impl FakeBucket {
    pub fn insert(&self, key: &str, body: &[u8], last_modified: u64) {
        self.objects
            .lock()
            .unwrap()
            .insert(key.to_owned(), (body.to_vec(), at(last_modified)));
    }

    pub fn contains(&self, key: &str) -> bool {
        self.objects.lock().unwrap().contains_key(key)
    }

    pub fn body(&self, key: &str) -> Option<Vec<u8>> {
        self.objects
            .lock()
            .unwrap()
            .get(key)
            .map(|(body, _)| body.clone())
    }

    pub fn keys(&self) -> Vec<String> {
        self.objects.lock().unwrap().keys().cloned().collect()
    }

    pub fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    pub fn writes(&self) -> Vec<String> {
        self.log()
            .into_iter()
            .filter(|line| line.starts_with("put ") || line.starts_with("delete "))
            .collect()
    }

    pub fn fail_delete(&self, key: &str) {
        self.failing_deletes.lock().unwrap().insert(key.to_owned());
    }

    pub fn fail_get(&self, key: &str) {
        self.failing_gets.lock().unwrap().insert(key.to_owned());
    }

    pub fn fail_check(&self, error: BucketError) {
        *self.check_error.lock().unwrap() = Some(error);
    }

    pub fn list_calls(&self) -> Vec<(String, Option<String>, usize)> {
        self.list_calls.lock().unwrap().clone()
    }

    fn note(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }
}

impl Bucket for FakeBucket {
    fn check(&self) -> BucketFuture<'_, ()> {
        Box::pin(async move {
            match self.check_error.lock().unwrap().clone() {
                Some(error) => Err(error),
                None => Ok(()),
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
            self.list_calls.lock().unwrap().push((
                prefix.to_owned(),
                after.map(str::to_owned),
                limit,
            ));
            let objects = self.objects.lock().unwrap();
            Ok(objects
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .filter(|(key, _)| after.is_none_or(|after| key.as_str() > after))
                .take(limit)
                .map(|(key, (_, last_modified))| ObjectInfo {
                    key: key.clone(),
                    last_modified: *last_modified,
                })
                .collect())
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> BucketFuture<'a, Option<Bytes>> {
        Box::pin(async move {
            self.note(format!("get {key}"));
            if self.failing_gets.lock().unwrap().contains(key) {
                return Err(BucketError::Call("scripted get failure".to_owned()));
            }
            Ok(self.body(key).map(Bytes::from))
        })
    }

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> BucketFuture<'a, ()> {
        Box::pin(async move {
            self.note(format!("put {key}"));
            self.insert(key, &body, 0);
            Ok(())
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BucketFuture<'a, ()> {
        Box::pin(async move {
            self.note(format!("delete {key}"));
            if self.failing_deletes.lock().unwrap().contains(key) {
                return Err(BucketError::Call("scripted delete failure".to_owned()));
            }
            self.objects.lock().unwrap().remove(key);
            Ok(())
        })
    }
}
