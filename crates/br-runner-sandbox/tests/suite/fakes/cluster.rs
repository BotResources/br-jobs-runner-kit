use std::collections::{BTreeMap, VecDeque};
use std::io::Cursor;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};

use br_runner_sandbox::{
    Cluster, ClusterError, ClusterFuture, ExecIo, ExecRequest, LabelSelector, PodEvent, PodView,
    ReplicaSetView,
};
use futures::StreamExt;
use futures::stream::BoxStream;
use serde_json::Value;
use tokio::io::AsyncWrite;
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Script {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: Option<i32>,
    pub wait_for_stdin_close: bool,
    pub hang: bool,
    pub refuse: Option<ClusterError>,
}

impl Script {
    pub fn exit(code: i32) -> Self {
        Self {
            code: Some(code),
            ..Self::default()
        }
    }

    pub fn out(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            stdout: stdout.into(),
            code: Some(0),
            ..Self::default()
        }
    }

    pub fn like_bsdtar_extract() -> Self {
        Self {
            code: Some(0),
            wait_for_stdin_close: true,
            ..Self::default()
        }
    }

    pub fn no_status() -> Self {
        Self::default()
    }

    pub fn hanging() -> Self {
        Self {
            hang: true,
            ..Self::default()
        }
    }

    pub fn with_stderr(mut self, stderr: &str) -> Self {
        self.stderr = stderr.as_bytes().to_vec();
        self
    }
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    closed: bool,
}

struct StdinCapture {
    capture: Arc<Mutex<Capture>>,
    closed: Arc<Notify>,
}

impl AsyncWrite for StdinCapture {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.capture
            .lock()
            .expect("capture")
            .bytes
            .extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.capture.lock().expect("capture").closed = true;
        self.closed.notify_one();
        Poll::Ready(Ok(()))
    }
}

pub struct ExecRecord {
    pub request: ExecRequest,
    capture: Arc<Mutex<Capture>>,
}

impl ExecRecord {
    pub fn stdin(&self) -> Vec<u8> {
        self.capture.lock().expect("capture").bytes.clone()
    }

    pub fn stdin_closed(&self) -> bool {
        self.capture.lock().expect("capture").closed
    }
}

#[derive(Default)]
struct State {
    pods: BTreeMap<String, PodView>,
    scripts: VecDeque<Script>,
    execs: Vec<Arc<ExecRecord>>,
    patches: Vec<(String, Value)>,
    fail_patch: bool,
}

#[derive(Clone, Default)]
pub struct FakeCluster {
    state: Arc<Mutex<State>>,
}

impl FakeCluster {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("the fake cluster lock")
    }

    pub fn add_pod(&self, pod: PodView) {
        self.lock().pods.insert(pod.name.clone(), pod);
    }

    pub fn push_script(&self, script: Script) {
        self.lock().scripts.push_back(script);
    }

    pub fn fail_patches(&self) {
        self.lock().fail_patch = true;
    }

    pub fn execs(&self) -> Vec<Arc<ExecRecord>> {
        self.lock().execs.clone()
    }

    pub fn patches(&self) -> Vec<(String, Value)> {
        self.lock().patches.clone()
    }
}

pub fn pod(name: &str) -> PodView {
    PodView {
        name: name.to_owned(),
        resource_version: "1".to_owned(),
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        phase: Some("Running".to_owned()),
        ready_condition: true,
        deleting: false,
    }
}

impl Cluster for FakeCluster {
    fn list_pods<'a>(&'a self, selector: &'a LabelSelector) -> ClusterFuture<'a, Vec<PodView>> {
        Box::pin(async move {
            Ok(self
                .lock()
                .pods
                .values()
                .filter(|pod| selector.matches(&pod.labels))
                .cloned()
                .collect())
        })
    }

    fn get_pod<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, Option<PodView>> {
        Box::pin(async move { Ok(self.lock().pods.get(name).cloned()) })
    }

    fn patch_pod<'a>(&'a self, name: &'a str, patch: &'a Value) -> ClusterFuture<'a, PodView> {
        Box::pin(async move {
            let mut state = self.lock();
            if state.fail_patch {
                return Err(ClusterError::Call(
                    "the fake cluster refuses patches".to_owned(),
                ));
            }
            state.patches.push((name.to_owned(), patch.clone()));
            state
                .pods
                .get(name)
                .cloned()
                .ok_or_else(|| ClusterError::Gone(name.to_owned()))
        })
    }

    fn create_pod<'a>(&'a self, _: &'a Value) -> ClusterFuture<'a, PodView> {
        Box::pin(async move { Ok(pod("created")) })
    }

    fn delete_pod<'a>(&'a self, _: &'a str) -> ClusterFuture<'a, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn replica_set<'a>(&'a self, _: &'a str) -> ClusterFuture<'a, Option<ReplicaSetView>> {
        Box::pin(async move { Ok(None) })
    }

    fn watch_pods<'a>(&'a self, _: &'a LabelSelector) -> BoxStream<'a, PodEvent> {
        futures::stream::empty().boxed()
    }

    fn exec<'a>(&'a self, request: ExecRequest) -> ClusterFuture<'a, ExecIo> {
        Box::pin(async move {
            let script = self.lock().scripts.pop_front().unwrap_or_default();
            if let Some(error) = script.refuse.clone() {
                return Err(error);
            }
            let capture = Arc::new(Mutex::new(Capture::default()));
            let closed = Arc::new(Notify::new());
            let wants_stdin = request.stdin;
            self.lock().execs.push(Arc::new(ExecRecord {
                request,
                capture: capture.clone(),
            }));
            let wait = closed.clone();
            let (hang, wait_for_close, code) =
                (script.hang, script.wait_for_stdin_close, script.code);
            let status = Box::pin(async move {
                if hang {
                    std::future::pending::<()>().await;
                }
                if wait_for_close {
                    wait.notified().await;
                }
                code
            });
            Ok(ExecIo {
                stdin: wants_stdin.then(|| Box::new(StdinCapture { capture, closed }) as _),
                stdout: Box::new(Cursor::new(script.stdout)),
                stderr: Box::new(Cursor::new(script.stderr)),
                status,
            })
        })
    }
}
