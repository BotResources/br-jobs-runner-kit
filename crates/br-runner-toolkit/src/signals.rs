use crate::error::{Error, ErrorCode};

#[cfg(unix)]
pub(crate) struct Signals {
    terminate: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    pub(crate) fn install() -> Result<Self, Error> {
        use tokio::signal::unix::{SignalKind, signal};
        let not_listened = |error: std::io::Error| {
            Error::new(
                ErrorCode::SignalsNotListened,
                format!("the process signals cannot be listened to: {error}"),
            )
        };
        Ok(Self {
            terminate: signal(SignalKind::terminate()).map_err(not_listened)?,
            interrupt: signal(SignalKind::interrupt()).map_err(not_listened)?,
        })
    }

    pub(crate) async fn received(&mut self) {
        tokio::select! {
            _ = self.terminate.recv() => {}
            _ = self.interrupt.recv() => {}
        }
    }
}

#[cfg(windows)]
pub(crate) struct Signals {
    interrupt: tokio::signal::windows::CtrlC,
}

#[cfg(windows)]
impl Signals {
    pub(crate) fn install() -> Result<Self, Error> {
        let interrupt = tokio::signal::windows::ctrl_c().map_err(|error| {
            Error::new(
                ErrorCode::SignalsNotListened,
                format!("the process signals cannot be listened to: {error}"),
            )
        })?;
        Ok(Self { interrupt })
    }

    pub(crate) async fn received(&mut self) {
        let _ = self.interrupt.recv().await;
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) struct Signals;

#[cfg(not(any(unix, windows)))]
impl Signals {
    pub(crate) fn install() -> Result<Self, Error> {
        Err(Error::new(
            ErrorCode::SignalsNotListened,
            "the process signals cannot be listened to on this platform",
        ))
    }

    pub(crate) async fn received(&mut self) {
        std::future::pending::<()>().await;
    }
}
