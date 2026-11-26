use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use br_runner_sandbox::{PartStream, PartUpload, PutOutcome, Store, StoreError, StoreFuture};
use bytes::Bytes;
use futures::StreamExt;

#[derive(Default)]
struct State {
    objects: BTreeMap<String, Bytes>,
    log: Vec<String>,
    failures: Vec<(&'static str, String)>,
    aborted: Vec<String>,
    rival: Option<(String, Bytes)>,
    size_lies: Vec<(String, u64)>,
    stream_chunk: usize,
    on_stream: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[derive(Clone, Default)]
pub struct FakeStore {
    state: Arc<Mutex<State>>,
}

fn call_error() -> StoreError {
    StoreError::Call("the fake store was told to fail".to_owned())
}

impl FakeStore {
    pub fn new() -> Self {
        let store = Self::default();
        store.lock().stream_chunk = 1000;
        store
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("the fake store lock")
    }

    pub fn insert(&self, key: &str, bytes: impl Into<Bytes>) {
        self.lock().objects.insert(key.to_owned(), bytes.into());
    }

    pub fn remove(&self, key: &str) {
        self.lock().objects.remove(key);
    }

    pub fn object(&self, key: &str) -> Option<Bytes> {
        self.lock().objects.get(key).cloned()
    }

    pub fn keys(&self) -> Vec<String> {
        self.lock().objects.keys().cloned().collect()
    }

    pub fn log(&self) -> Vec<String> {
        self.lock().log.clone()
    }

    pub fn clear_log(&self) {
        self.lock().log.clear();
    }

    pub fn aborted(&self) -> Vec<String> {
        self.lock().aborted.clone()
    }

    pub fn fail(&self, operation: &'static str, key_part: &str) {
        self.lock().failures.push((operation, key_part.to_owned()));
    }

    pub fn rival_before_create(&self, key: &str, bytes: impl Into<Bytes>) {
        self.lock().rival = Some((key.to_owned(), bytes.into()));
    }

    pub fn lie_about_size(&self, key_part: &str, size: u64) {
        self.lock().size_lies.push((key_part.to_owned(), size));
    }

    pub fn stream_in_chunks_of(&self, size: usize) {
        self.lock().stream_chunk = size;
    }

    pub fn on_stream(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.lock().on_stream = Some(Arc::new(hook));
    }

    fn enter(&self, operation: &'static str, key: &str) -> Result<(), StoreError> {
        let mut state = self.lock();
        state.log.push(format!("{operation} {key}"));
        let fails = state
            .failures
            .iter()
            .any(|(failing, part)| *failing == operation && key.contains(part.as_str()));
        if fails { Err(call_error()) } else { Ok(()) }
    }
}

struct FakeUpload {
    store: FakeStore,
    key: String,
    buffer: Vec<u8>,
}

impl PartUpload for FakeUpload {
    fn write(&mut self, chunk: Bytes) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            self.store.enter("upload_write", &self.key)?;
            self.buffer.extend_from_slice(&chunk);
            Ok(())
        })
    }

    fn finish(self: Box<Self>) -> StoreFuture<'static, ()> {
        Box::pin(async move {
            self.store.enter("upload_finish", &self.key)?;
            self.store.insert(&self.key, Bytes::from(self.buffer));
            Ok(())
        })
    }

    fn abort(self: Box<Self>) -> StoreFuture<'static, ()> {
        Box::pin(async move {
            self.store.enter("upload_abort", &self.key)?;
            self.store.lock().aborted.push(self.key.clone());
            Ok(())
        })
    }
}

impl Store for FakeStore {
    fn get<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<Bytes>> {
        Box::pin(async move {
            self.enter("get", key)?;
            Ok(self.object(key))
        })
    }

    fn get_stream<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<PartStream>> {
        Box::pin(async move {
            self.enter("get_stream", key)?;
            let hook = self.lock().on_stream.clone();
            if let Some(hook) = hook {
                hook();
            }
            let chunk = self.lock().stream_chunk.max(1);
            Ok(self.object(key).map(|bytes| {
                let pieces: Vec<Result<Bytes, StoreError>> = bytes
                    .chunks(chunk)
                    .map(|piece| Ok(Bytes::copy_from_slice(piece)))
                    .collect();
                futures::stream::iter(pieces).boxed()
            }))
        })
    }

    fn size<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Option<u64>> {
        Box::pin(async move {
            self.enter("size", key)?;
            let lie = self
                .lock()
                .size_lies
                .iter()
                .find(|(part, _)| key.contains(part.as_str()))
                .map(|(_, size)| *size);
            Ok(lie.or_else(|| self.object(key).map(|bytes| bytes.len() as u64)))
        })
    }

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.enter("put", key)?;
            self.insert(key, body);
            Ok(())
        })
    }

    fn put_if_absent<'a>(&'a self, key: &'a str, body: Bytes) -> StoreFuture<'a, PutOutcome> {
        Box::pin(async move {
            self.enter("put_if_absent", key)?;
            let rival = self.lock().rival.take();
            if let Some((rival_key, bytes)) = rival {
                self.insert(&rival_key, bytes);
            }
            if self.object(key).is_some() {
                return Ok(PutOutcome::AlreadyExists);
            }
            self.insert(key, body);
            Ok(PutOutcome::Created)
        })
    }

    fn start_upload<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Box<dyn PartUpload>> {
        Box::pin(async move {
            self.enter("upload_start", key)?;
            Ok(Box::new(FakeUpload {
                store: self.clone(),
                key: key.to_owned(),
                buffer: Vec::new(),
            }) as Box<dyn PartUpload>)
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> StoreFuture<'a, Vec<String>> {
        Box::pin(async move {
            self.enter("list", prefix)?;
            Ok(self
                .keys()
                .into_iter()
                .filter(|key| key.starts_with(prefix))
                .collect())
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.enter("delete", key)?;
            self.lock().objects.remove(key);
            Ok(())
        })
    }
}
