use std::io::{Error, ErrorKind, Result};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, ReadBuf};
use tokio::time::Instant;

#[derive(Clone, Default)]
pub struct StdoutModel {
    pub grace: Option<Duration>,
    pub cut_after: Option<usize>,
    pub yield_each_read: bool,
    pub stall_after: Option<usize>,
    pub fail_after: Option<usize>,
    pub panic_after: Option<usize>,
    pub delivered: Arc<AtomicUsize>,
    pub dropped: Arc<AtomicBool>,
}

impl StdoutModel {
    pub fn lossy_after(grace: Duration) -> Self {
        Self {
            grace: Some(grace),
            ..Self::default()
        }
    }

    pub fn cut_after(bytes: usize) -> Self {
        Self {
            cut_after: Some(bytes),
            ..Self::default()
        }
    }

    pub fn yielding() -> Self {
        Self {
            yield_each_read: true,
            ..Self::default()
        }
    }

    pub fn stalling_after(bytes: usize) -> Self {
        Self {
            stall_after: Some(bytes),
            yield_each_read: true,
            ..Self::default()
        }
    }

    pub fn failing_after(bytes: usize) -> Self {
        Self {
            fail_after: Some(bytes),
            yield_each_read: true,
            ..Self::default()
        }
    }

    pub fn panicking_after(bytes: usize) -> Self {
        Self {
            panic_after: Some(bytes),
            ..Self::default()
        }
    }

    pub fn dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }

    pub fn delivered(&self) -> usize {
        self.delivered.load(Ordering::SeqCst)
    }
}

pub struct ModelStdout {
    data: Vec<u8>,
    position: usize,
    deadline: Option<Instant>,
    end: usize,
    yield_each_read: bool,
    yielded: bool,
    stall_after: Option<usize>,
    fail_after: Option<usize>,
    panic_after: Option<usize>,
    delivered: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

impl ModelStdout {
    pub fn new(data: Vec<u8>, model: StdoutModel) -> Self {
        Self {
            position: 0,
            end: model
                .cut_after
                .map_or(data.len(), |cut| cut.min(data.len())),
            data,
            deadline: model.grace.map(|grace| Instant::now() + grace),
            yield_each_read: model.yield_each_read,
            yielded: false,
            stall_after: model.stall_after,
            fail_after: model.fail_after,
            panic_after: model.panic_after,
            delivered: model.delivered,
            dropped: model.dropped,
        }
    }
}

impl Drop for ModelStdout {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl AsyncRead for ModelStdout {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let this = self.get_mut();
        if this
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Poll::Ready(Ok(()));
        }
        if this.yield_each_read && !this.yielded {
            this.yielded = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        this.yielded = false;
        if this.stall_after.is_some_and(|limit| this.position >= limit) {
            return Poll::Pending;
        }
        if this.fail_after.is_some_and(|limit| this.position >= limit) {
            return Poll::Ready(Err(Error::new(ErrorKind::ConnectionReset, "reset")));
        }
        assert!(
            !this.panic_after.is_some_and(|limit| this.position >= limit),
            "the fake stdout was told to panic"
        );
        let take = buf.remaining().min(this.end - this.position);
        buf.put_slice(&this.data[this.position..this.position + take]);
        this.position += take;
        this.delivered.fetch_add(take, Ordering::SeqCst);
        Poll::Ready(Ok(()))
    }
}
