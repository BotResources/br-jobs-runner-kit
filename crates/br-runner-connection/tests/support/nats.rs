use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct Server {
    child: Option<Child>,
    port: u16,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn spawn(port: u16) -> Child {
    let binary = std::env::var("NATS_SERVER_BIN").unwrap_or_else(|_| "nats-server".to_owned());
    Command::new(binary)
        .args(["-a", "127.0.0.1", "-p", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("nats-server starts")
}

impl Server {
    pub fn start() -> Self {
        let port = free_port();
        Self {
            child: Some(spawn(port)),
            port,
        }
    }

    pub fn url(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn restart(&mut self) {
        self.kill();
        self.child = Some(spawn(self.port));
    }

    pub async fn client(&self) -> async_nats::Client {
        for _ in 0..200 {
            if let Ok(client) = async_nats::connect(self.url()).await {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("nats-server did not start");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}
