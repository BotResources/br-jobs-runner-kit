#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

const EMPTY_LISTING: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Name>saves</Name><Prefix></Prefix><KeyCount>0</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated></ListBucketResult>";

fn reply(method: &str, target: &str) -> (u16, &'static str, String) {
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    if path.starts_with("/saves") {
        (200, "application/xml", EMPTY_LISTING.to_owned())
    } else if path == "/api/v1/namespaces" {
        let namespaces = json!({"apiVersion": "v1", "kind": "NamespaceList",
            "metadata": {"resourceVersion": "1"}, "items": [{"metadata": {"name": "sbx"}}]});
        (200, "application/json", namespaces.to_string())
    } else if path.ends_with("/pods") {
        let pods = json!({"apiVersion": "v1", "kind": "PodList",
            "metadata": {"resourceVersion": "1"}, "items": []});
        (200, "application/json", pods.to_string())
    } else if path.contains("/leases") && method == "POST" {
        let lease = json!({"apiVersion": "coordination.k8s.io/v1", "kind": "Lease",
            "metadata": {"name": "br-sandbox-keeper", "namespace": "sys"}});
        (201, "application/json", lease.to_string())
    } else {
        let status = json!({"apiVersion": "v1", "kind": "Status", "status": "Failure",
            "message": "not found", "reason": "NotFound", "code": 404});
        (404, "application/json", status.to_string())
    }
}

fn answer(mut stream: TcpStream) {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => head.extend_from_slice(&chunk[..read]),
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut request_line = text.lines().next().unwrap_or_default().split(' ');
    let method = request_line.next().unwrap_or_default();
    let target = request_line.next().unwrap_or_default();
    if method == "GET" && target.contains("/pods?") && target.contains("watch=true") {
        let opened = "HTTP/1.1 200 X\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
        if stream.write_all(opened.as_bytes()).is_ok() {
            thread::sleep(Duration::from_secs(3600));
        }
        return;
    }
    let (status, kind, body) = reply(method, target);
    let message = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(message.as_bytes()).ok();
}

fn serve_cluster_and_store() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || answer(stream));
        }
    });
    port
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn ready(port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let request = "GET /readyz HTTP/1.1\r\nHost: keeper\r\nConnection: close\r\n\r\n";
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok();
    answer.starts_with("HTTP/1.1 200")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("keeper-sigterm-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for name in ["config.json", "kubeconfig"] {
            std::fs::remove_file(self.0.join(name)).ok();
        }
        std::fs::remove_dir(&self.0).ok();
    }
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn the_binary_that_serves_ends_with_code_0_on_sigterm() {
    let backend = serve_cluster_and_store();
    let health = free_port();
    let scratch = Scratch::new();
    let configuration = scratch.file(
        "config.json",
        &json!({
            "store_bucket": "saves",
            "store_endpoint": format!("http://127.0.0.1:{backend}"),
            "store_region": "eu",
            "store_path_style": true,
            "listen_address": format!("127.0.0.1:{health}"),
            "lease_namespace": "sys",
            "save_ttl_seconds": 1000,
            "skew_margin_seconds": 60,
            "pools": [{"id": "p", "namespace": "sbx", "replicaset": "warm-p",
                "min_replicas": 1, "max_replicas": 20}]
        })
        .to_string(),
    );
    let kubeconfig = scratch.file(
        "kubeconfig",
        &json!({
            "apiVersion": "v1",
            "kind": "Config",
            "clusters": [{"name": "c", "cluster": {"server": format!("http://127.0.0.1:{backend}")}}],
            "users": [{"name": "u", "user": {}}],
            "contexts": [{"name": "x", "context": {"cluster": "c", "user": "u"}}],
            "current-context": "x"
        })
        .to_string(),
    );
    let mut keeper = Running(
        Command::new(env!("CARGO_BIN_EXE_br-sandbox-keeper"))
            .env_clear()
            .env("BR_SANDBOX_CONFIG", &configuration)
            .env("KUBECONFIG", &kubeconfig)
            .env("AWS_ACCESS_KEY_ID", "test-access-key")
            .env("AWS_SECRET_ACCESS_KEY", "test-secret-key")
            .env("HOSTNAME", "keeper-test")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(
        wait_until(Duration::from_secs(60), || ready(health)),
        "the keeper never became ready"
    );
    let sent = Command::new("kill")
        .args(["-TERM", &keeper.0.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let mut status = None;
    assert!(
        wait_until(Duration::from_secs(30), || {
            status = keeper.0.try_wait().unwrap();
            status.is_some()
        }),
        "the keeper did not end after SIGTERM"
    );
    assert_eq!(status.unwrap().code(), Some(0));
}
