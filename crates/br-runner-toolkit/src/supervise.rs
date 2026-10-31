use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;
use tokio::task::JoinHandle;

use crate::panic::text_of;
use crate::shared::Shared;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Restart {
    AtOnce,
    AfterBeat,
}

pub(crate) fn supervised<F, Fut>(
    shared: Arc<Shared>,
    name: &'static str,
    restart: Restart,
    make: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match AssertUnwindSafe(make()).catch_unwind().await {
                Ok(()) => return,
                Err(payload) => {
                    tracing::error!(
                        task = name,
                        panic = %text_of(payload),
                        "a task of the loop ended by a panic and is started again"
                    );
                    match restart {
                        Restart::AtOnce => tokio::task::yield_now().await,
                        Restart::AfterBeat => {
                            tokio::select! {
                                () = shared.next_beat() => {}
                                () = shared.drain.cancelled() => {}
                            }
                        }
                    }
                }
            }
        }
    })
}
