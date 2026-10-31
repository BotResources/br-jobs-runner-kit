use super::*;

pub struct Rig {
    pub queue: DeliveryQueue,
    pub sender: Arc<ScriptedSender>,
    pub observer: Arc<Recorder>,
    pub started: WriteEnd,
}

pub fn settings_with(
    observer: Arc<dyn QueueObserver>,
    outcome_wait: Duration,
    retry_wait: Duration,
) -> QueueSettings {
    QueueSettings {
        start_id: id(START),
        outcome_wait,
        retry_wait,
        observer,
    }
}

impl Rig {
    pub fn new() -> Self {
        Self::with(ScriptedSender::new())
    }

    pub fn with(sender: Arc<ScriptedSender>) -> Self {
        Self::timed(
            sender,
            Duration::from_secs(86_400),
            Duration::from_millis(1),
        )
    }

    pub fn timed(
        sender: Arc<ScriptedSender>,
        outcome_wait: Duration,
        retry_wait: Duration,
    ) -> Self {
        let observer = Arc::new(Recorder::default());
        let settings = settings_with(observer.clone(), outcome_wait, retry_wait);
        let (queue, started) = DeliveryQueue::start(sender.clone(), settings, id(RUN))
            .unwrap_or_else(|e| panic!("a queue: {e}"));
        Self {
            queue,
            sender,
            observer,
            started,
        }
    }

    pub fn hand(&self, write: Write) -> WriteEnd {
        self.queue.writer().hand_over(write)
    }

    pub fn hold(&self, marker: Write, follow: Option<Write>) -> (oneshot::Sender<Gate>, WriteEnd) {
        let (tx, gate) = gate();
        (tx, self.queue.writer().hold(marker, follow, gate))
    }
}

pub fn gate() -> (oneshot::Sender<Gate>, BoxFuture<'static, Gate>) {
    let (tx, rx) = oneshot::channel();
    (
        tx,
        Box::pin(async move { rx.await.unwrap_or(Gate::Dropped) }),
    )
}

pub struct DropFlag(pub Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub fn tracked_gate() -> (
    oneshot::Sender<Gate>,
    BoxFuture<'static, Gate>,
    Arc<AtomicBool>,
) {
    let (tx, rx) = oneshot::channel();
    let flag = Arc::new(AtomicBool::new(false));
    let held = DropFlag(Arc::clone(&flag));
    let future: BoxFuture<'static, Gate> = Box::pin(async move {
        let _held = held;
        rx.await.unwrap_or(Gate::Dropped)
    });
    (tx, future, flag)
}

pub async fn settle() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

pub fn ready(end: &mut WriteEnd) -> Option<WriteResult> {
    end.now_or_never()
}
