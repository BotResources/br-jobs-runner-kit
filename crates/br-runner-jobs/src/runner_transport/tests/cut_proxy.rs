use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

pub(super) struct Proxy {
    address: String,
    discarding: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    listener: JoinHandle<()>,
}

impl Proxy {
    pub(super) async fn start(target: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let address = listener.local_addr().expect("an address").to_string();
        let discarding = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(Mutex::new(Vec::new()));
        let accepting = tokio::spawn({
            let (discarding, connections) = (discarding.clone(), connections.clone());
            async move {
                while let Ok((client, _)) = listener.accept().await {
                    let Ok(server) = TcpStream::connect(&target).await else {
                        continue;
                    };
                    let (mut from_client, mut to_client) = client.into_split();
                    let (mut from_server, mut to_server) = server.into_split();
                    let upstream = tokio::spawn(async move {
                        tokio::io::copy(&mut from_client, &mut to_server).await.ok();
                    });
                    let discarding = discarding.clone();
                    let downstream = tokio::spawn(async move {
                        let mut buffer = vec![0u8; 16 * 1024];
                        while let Ok(read) = from_server.read(&mut buffer).await {
                            if read == 0 {
                                break;
                            }
                            if !discarding.load(Ordering::SeqCst)
                                && to_client.write_all(&buffer[..read]).await.is_err()
                            {
                                break;
                            }
                        }
                    });
                    connections
                        .lock()
                        .expect("the connections")
                        .extend([upstream, downstream]);
                }
            }
        });
        Self {
            address,
            discarding,
            connections,
            listener: accepting,
        }
    }

    pub(super) fn url(&self) -> String {
        self.address.clone()
    }

    pub(super) fn discard_what_the_server_sends(&self, on: bool) {
        self.discarding.store(on, Ordering::SeqCst);
    }

    pub(super) fn cut(&self) {
        for connection in self.connections.lock().expect("the connections").drain(..) {
            connection.abort();
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.listener.abort();
        self.cut();
    }
}
