use super::*;

#[derive(Clone)]
pub struct Latch(Arc<Semaphore>);

impl Latch {
    pub fn new() -> Self {
        Self(Arc::new(Semaphore::new(0)))
    }

    pub fn open(&self) {
        self.0.add_permits(1);
    }

    async fn wait(&self) {
        if let Ok(permit) = self.0.acquire().await {
            permit.forget();
        }
    }
}

#[derive(Clone)]
pub enum Step {
    Reply(Result<(), Error>),
    Hang,
    Panic,
    After(Latch, Result<(), Error>),
}

struct Rule {
    steps: VecDeque<Step>,
    fallback: Step,
}

struct InFlight {
    finished: bool,
    cancelled: Arc<AtomicUsize>,
}

impl InFlight {
    fn finish(mut self) {
        self.finished = true;
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if !self.finished {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
        }
    }
}

pub struct ScriptedSender {
    by_name: Mutex<HashMap<WriteName, Rule>>,
    by_kind: Mutex<HashMap<WriteKind, Rule>>,
    attempts: Mutex<Vec<(Write, tokio::time::Instant)>>,
    count: watch::Sender<usize>,
    cancelled: Arc<AtomicUsize>,
    sessions: AtomicUsize,
}

impl ScriptedSender {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            by_name: Mutex::new(HashMap::new()),
            by_kind: Mutex::new(HashMap::new()),
            attempts: Mutex::new(Vec::new()),
            count: watch::channel(0).0,
            cancelled: Arc::new(AtomicUsize::new(0)),
            sessions: AtomicUsize::new(0),
        })
    }

    pub fn script(&self, kind: WriteKind, steps: Vec<Step>) {
        self.by_kind.lock().unwrap().insert(
            kind,
            Rule {
                steps: steps.into(),
                fallback: Step::Reply(Ok(())),
            },
        );
    }

    pub fn always(&self, kind: WriteKind, step: Step) {
        self.by_kind.lock().unwrap().insert(
            kind,
            Rule {
                steps: VecDeque::new(),
                fallback: step,
            },
        );
    }

    pub fn script_write(&self, write: &Write, steps: Vec<Step>) {
        self.by_name.lock().unwrap().insert(
            write.name(),
            Rule {
                steps: steps.into(),
                fallback: Step::Reply(Ok(())),
            },
        );
    }

    fn next_step(&self, write: &Write) -> Step {
        let name = write.name();
        let pull = |rule: &mut Rule| {
            rule.steps
                .pop_front()
                .unwrap_or_else(|| rule.fallback.clone())
        };
        if let Some(rule) = self.by_name.lock().unwrap().get_mut(&name) {
            return pull(rule);
        }
        if let Some(rule) = self.by_kind.lock().unwrap().get_mut(&name.kind) {
            return pull(rule);
        }
        Step::Reply(Ok(()))
    }

    pub fn attempts(&self) -> Vec<Write> {
        self.attempts
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.0.clone())
            .collect()
    }

    pub fn times(&self) -> Vec<tokio::time::Instant> {
        self.attempts.lock().unwrap().iter().map(|a| a.1).collect()
    }

    pub fn kinds(&self) -> Vec<WriteKind> {
        self.attempts().iter().map(|w| w.name().kind).collect()
    }

    pub fn attempts_of(&self, kind: WriteKind) -> Vec<Write> {
        self.attempts()
            .into_iter()
            .filter(|w| w.name().kind == kind)
            .collect()
    }

    pub fn sessions(&self) -> usize {
        self.sessions.load(Ordering::SeqCst)
    }

    pub fn cancelled(&self) -> usize {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub async fn attempts_reached(&self, n: usize) {
        let mut rx = self.count.subscribe();
        let _ = rx.wait_for(|count| *count >= n).await;
    }
}

impl WriteSender for ScriptedSender {
    fn session<'a>(&'a self, write: &'a Write) -> Box<dyn WriteSession + 'a> {
        self.sessions.fetch_add(1, Ordering::SeqCst);
        Box::new(Direct {
            sender: self,
            write,
        })
    }

    fn send<'a>(&'a self, write: &'a Write) -> BoxFuture<'a, Result<(), Error>> {
        let step = self.next_step(write);
        self.attempts
            .lock()
            .unwrap()
            .push((write.clone(), tokio::time::Instant::now()));
        self.count.send_modify(|count| *count += 1);
        let guard = InFlight {
            finished: false,
            cancelled: Arc::clone(&self.cancelled),
        };
        Box::pin(async move {
            let result = match step {
                Step::Reply(result) => result,
                Step::Hang => std::future::pending().await,
                Step::Panic => panic!("the sender panicked"),
                Step::After(latch, result) => {
                    latch.wait().await;
                    result
                }
            };
            guard.finish();
            result
        })
    }
}

#[derive(Default)]
pub struct Recorder {
    reports: Mutex<Vec<QueueReport>>,
}

impl Recorder {
    pub fn reports(&self) -> Vec<QueueReport> {
        self.reports.lock().unwrap().clone()
    }

    pub fn retrying(&self) -> Vec<(WriteName, Error)> {
        self.reports()
            .into_iter()
            .filter_map(|r| match r {
                QueueReport::Retrying { write, error } => Some((write, error)),
                _ => None,
            })
            .collect()
    }

    pub fn removed(&self) -> Vec<(WriteName, Removal)> {
        self.reports()
            .into_iter()
            .filter_map(|r| match r {
                QueueReport::Removed { write, removal } => Some((write, removal)),
                _ => None,
            })
            .collect()
    }

    pub fn delivered(&self) -> Vec<WriteName> {
        self.reports()
            .into_iter()
            .filter_map(|r| match r {
                QueueReport::Delivered { write } => Some(write),
                _ => None,
            })
            .collect()
    }
}

impl QueueObserver for Recorder {
    fn report(&self, report: &QueueReport) {
        self.reports.lock().unwrap().push(report.clone());
    }
}
