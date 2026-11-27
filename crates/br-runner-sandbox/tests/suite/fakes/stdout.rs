use std::io::Result;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, ReadBuf};
use tokio::time::Instant;

#[derive(Clone, Default)]
pub struct StdoutModel {
    pub grace: Option<Duration>,
    pub cut_after: Option<usize>,
    pub yield_each_read: bool,
    pub delivered: Arc<AtomicUsize>,
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
    delivered: Arc<AtomicUsize>,
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
            delivered: model.delivered,
        }
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
        let take = buf.remaining().min(this.end - this.position);
        buf.put_slice(&this.data[this.position..this.position + take]);
        this.position += take;
        this.delivered.fetch_add(take, Ordering::SeqCst);
        Poll::Ready(Ok(()))
    }
}
