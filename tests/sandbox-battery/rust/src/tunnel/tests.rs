use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{Gate, GateMode};

async fn echo_upstream() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0u8; 256];
                while let Ok(read) = stream.read(&mut buffer).await {
                    if read == 0 || stream.write_all(&buffer[..read]).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    address
}

async fn gate(parse: bool) -> Gate {
    Gate::start(echo_upstream().await, parse).await.unwrap()
}

async fn connect(gate: &Gate) -> TcpStream {
    TcpStream::connect(("127.0.0.1", gate.port()))
        .await
        .unwrap()
}

async fn echoed(stream: &mut TcpStream) {
    stream.write_all(b"ping").await.unwrap();
    let mut buffer = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buffer, b"ping");
}

async fn wait_live(gate: &Gate, count: usize) {
    let end = Instant::now() + Duration::from_secs(5);
    while gate.live() != count {
        assert!(Instant::now() < end, "live connections: {}", gate.live());
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn is_closed(stream: &mut TcpStream) -> bool {
    let mut buffer = [0u8; 8];
    matches!(
        tokio::time::timeout(Duration::from_millis(30), stream.read(&mut buffer)).await,
        Ok(Ok(0) | Err(_))
    )
}

#[tokio::test]
async fn a_connection_open_before_refuse_is_closed_when_set_returns() {
    let gate = gate(false).await;
    let mut client = connect(&gate).await;
    echoed(&mut client).await;
    wait_live(&gate, 1).await;
    gate.set(GateMode::Refuse).await.unwrap();
    assert_eq!(gate.live(), 0);
    assert!(is_closed(&mut client).await);
}

#[tokio::test]
async fn a_connection_open_before_stall_is_closed_when_set_returns() {
    let gate = gate(false).await;
    let mut client = connect(&gate).await;
    echoed(&mut client).await;
    gate.set(GateMode::Stall).await.unwrap();
    assert_eq!(gate.live(), 0);
    assert!(is_closed(&mut client).await);
}

#[tokio::test]
async fn a_connection_made_while_refusing_is_closed() {
    let gate = gate(false).await;
    gate.set(GateMode::Refuse).await.unwrap();
    let mut client = connect(&gate).await;
    assert!(is_closed(&mut client).await);
}

#[tokio::test]
async fn a_connection_made_while_stalling_is_held_until_the_next_setting() {
    let gate = gate(false).await;
    gate.set(GateMode::Stall).await.unwrap();
    let mut client = connect(&gate).await;
    wait_live(&gate, 1).await;
    let mut buffer = [0u8; 1];
    assert!(
        tokio::time::timeout(Duration::from_millis(100), client.read(&mut buffer))
            .await
            .is_err()
    );
    gate.set(GateMode::Open).await.unwrap();
    assert_eq!(gate.live(), 0);
    assert!(is_closed(&mut client).await);
}

#[tokio::test]
async fn set_returns_at_once_when_no_connection_is_open() {
    let gate = gate(false).await;
    let started = Instant::now();
    gate.set(GateMode::Refuse).await.unwrap();
    assert!(started.elapsed() < Duration::from_millis(40));
}

#[tokio::test]
async fn a_paused_manifest_write_goes_on_after_the_next_setting_and_set_does_not_wait_for_it() {
    let gate = gate(true).await;
    gate.set(GateMode::PauseManifestWrite).await.unwrap();
    let mut client = connect(&gate).await;
    client
        .write_all(b"PUT /bucket/key/manifest.json HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    wait_live(&gate, 1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut buffer = [0u8; 1];
    assert!(
        tokio::time::timeout(Duration::from_millis(50), client.read(&mut buffer))
            .await
            .is_err()
    );
    gate.set(GateMode::Open).await.unwrap();
    assert_eq!(gate.live(), 1);
    let mut echoed_head = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut echoed_head))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&echoed_head, b"PUT ");
}
