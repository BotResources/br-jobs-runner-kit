use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::TcpStream;

use super::GateMode;
use super::state::{Setting, Slot, State};

const DENIED: &[u8] = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

async fn read_head(client: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16_384 || client.read(&mut byte).await.ok()? == 0 {
            return None;
        }
        head.push(byte[0]);
    }
    Some(head)
}

fn closing(head: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(head);
    let mut out = String::new();
    for line in text.split("\r\n").filter(|line| !line.is_empty()) {
        if !line.to_ascii_lowercase().starts_with("connection:") {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out.push_str("Connection: close\r\n\r\n");
    out.into_bytes()
}

fn writes_manifest(mode: GateMode) -> bool {
    matches!(
        mode,
        GateMode::DenyManifestWrite | GateMode::HoldManifestWrite | GateMode::PauseManifestWrite
    )
}

pub async fn serve(state: Arc<State>, client: TcpStream, upstream: String, parse: bool) {
    let (slot, setting) = state.enter();
    drive(&slot, setting, client, &upstream, parse).await;
}

async fn drive(slot: &Slot, setting: Setting, client: TcpStream, upstream: &str, parse: bool) {
    match setting.mode {
        GateMode::Refuse => {}
        GateMode::Stall => slot.until_behind().await,
        mode => relay(slot, mode, client, upstream, parse).await,
    }
}

async fn relay(slot: &Slot, mode: GateMode, mut client: TcpStream, upstream: &str, parse: bool) {
    let Some(Ok(mut server)) = slot.while_current(TcpStream::connect(upstream)).await else {
        return;
    };
    if parse && writes_manifest(mode) {
        let Some(Some(head)) = slot.while_current(read_head(&mut client)).await else {
            return;
        };
        let first = String::from_utf8_lossy(&head);
        let line = first.lines().next().unwrap_or("");
        if line.starts_with("PUT ") && line.contains("manifest.json") {
            match mode {
                GateMode::HoldManifestWrite => {
                    slot.until_behind().await;
                    return;
                }
                GateMode::PauseManifestWrite => {
                    slot.until_behind().await;
                    slot.rebind();
                }
                _ => {
                    let _ = slot.while_current(client.write_all(DENIED)).await;
                    return;
                }
            }
        }
        if !matches!(
            slot.while_current(server.write_all(&closing(&head))).await,
            Some(Ok(()))
        ) {
            return;
        }
    }
    let _ = slot
        .while_current(copy_bidirectional(&mut client, &mut server))
        .await;
}
