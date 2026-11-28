use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use tokio::signal::unix::{SignalKind, signal};

use super::{Registry, locked};
use crate::error::{BatteryError, Res};

const READY_SECONDS: u64 = 10;

pub fn exit_code(kind: SignalKind) -> i32 {
    if kind == SignalKind::interrupt() {
        130
    } else {
        143
    }
}

impl Registry {
    pub fn listen(self: &Arc<Self>, kinds: &[SignalKind]) -> Receiver<Result<(), String>> {
        let (ready, listening) = mpsc::channel();
        let kinds = kinds.to_vec();
        let registry = Arc::clone(self);
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready.send(Err(format!("no runtime: {error}")));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut streams = Vec::new();
                for kind in kinds {
                    match signal(kind) {
                        Ok(stream) => streams.push((kind, stream)),
                        Err(error) => {
                            let _ = ready.send(Err(format!("no handler for {kind:?}: {error}")));
                            return;
                        }
                    }
                }
                let _ = ready.send(Ok(()));
                let received =
                    futures::future::select_all(streams.iter_mut().map(|(kind, stream)| {
                        Box::pin(async move {
                            stream.recv().await;
                            *kind
                        })
                    }))
                    .await
                    .0;
                registry.restore_all();
                (registry.exit)(exit_code(received));
            });
        });
        listening
    }

    pub fn listen_once(self: &Arc<Self>) -> Res {
        let mut listening = locked(&self.listening);
        if *listening {
            return Ok(());
        }
        let ready = self.listen(&[SignalKind::terminate(), SignalKind::interrupt()]);
        match ready.recv_timeout(std::time::Duration::from_secs(READY_SECONDS)) {
            Ok(Ok(())) => {
                *listening = true;
                Ok(())
            }
            Ok(Err(reason)) => Err(BatteryError::Data {
                what: "the signal handlers of the restore".to_owned(),
                reason,
            }),
            Err(_) => Err(BatteryError::Hang {
                what: "the start of the signal handlers of the restore".to_owned(),
                seconds: READY_SECONDS,
            }),
        }
    }
}
