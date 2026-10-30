use std::process::{Child, Command, Stdio};
use std::time::Duration;

use uuid::Uuid;

pub struct Server {
    child: Child,
    port: u16,
    dir: std::path::PathBuf,
}

impl Server {
    pub fn start() -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        let dir = std::env::temp_dir().join(format!("br-runner-threads-nats-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("dir");
        let binary = std::env::var("NATS_SERVER_BIN").unwrap_or_else(|_| "nats-server".to_owned());
        let child = Command::new(binary)
            .args(["-js", "-a", "127.0.0.1", "-p", &port.to_string(), "-sd"])
            .arg(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nats-server starts");
        Self { child, port, dir }
    }

    pub async fn client(&self) -> async_nats::Client {
        for _ in 0..100 {
            if let Ok(client) = async_nats::connect(format!("127.0.0.1:{}", self.port)).await {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("nats-server did not start");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
