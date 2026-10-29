use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{Notify, oneshot};
use uuid::Uuid;

use super::{
    Answer, AnswerWait, BoxFuture, BusCommand, BusTransport, Confirmed, GraphqlTransport,
    HttpReply, OutboundChunk,
};
use crate::error::Error;

pub enum Reply {
    Answer(Answer),
    Manual,
    Never,
    Lost,
    Unreadable(String),
    Unavailable(String),
    PublishLost(String),
    Publishing(Arc<Notify>),
}

#[derive(Clone)]
pub enum ReadyMode {
    Open,
    Unavailable(String),
    Hold(Arc<Notify>),
}

#[derive(Clone)]
pub enum ChunkMode {
    Ack,
    Fail,
    Hold(Arc<Notify>),
}

enum Recorded {
    Held(Arc<Notify>),
    Now(Result<AnswerWait, Error>),
}

type Script = Box<dyn Fn(&BusCommand) -> Reply + Send + Sync>;

struct State {
    commands: Vec<BusCommand>,
    chunks: Vec<OutboundChunk>,
    script: Script,
    chunk_mode: ChunkMode,
    ready_mode: ReadyMode,
    manual: HashMap<Uuid, Vec<oneshot::Sender<Result<Answer, Error>>>>,
}

#[derive(Clone)]
pub struct MemoryBus {
    state: Arc<Mutex<State>>,
}

fn confirmed(command: &BusCommand) -> Reply {
    Reply::Answer(Answer::Confirmed(Confirmed {
        command_id: command.command_id,
        status: None,
    }))
}

impl Default for MemoryBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBus {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                commands: Vec::new(),
                chunks: Vec::new(),
                script: Box::new(confirmed),
                chunk_mode: ChunkMode::Ack,
                ready_mode: ReadyMode::Open,
                manual: HashMap::new(),
            })),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn answer_with(&self, script: impl Fn(&BusCommand) -> Reply + Send + Sync + 'static) {
        self.state().script = Box::new(script);
    }

    pub fn chunk_mode(&self, mode: ChunkMode) {
        self.state().chunk_mode = mode;
    }

    pub fn ready_mode(&self, mode: ReadyMode) {
        self.state().ready_mode = mode;
    }

    pub fn commands(&self) -> Vec<BusCommand> {
        self.state().commands.clone()
    }

    pub fn chunks(&self) -> Vec<OutboundChunk> {
        self.state().chunks.clone()
    }

    fn record(&self, command: BusCommand) -> Recorded {
        let mut state = self.state();
        let reply = (state.script)(&command);
        let id = command.command_id;
        match reply {
            Reply::Unavailable(detail) => Recorded::Now(Err(Error::Unavailable(detail))),
            Reply::PublishLost(detail) => {
                state.commands.push(command);
                Recorded::Now(Err(Error::AnswerLost(detail)))
            }
            Reply::Publishing(release) => {
                state.commands.push(command);
                Recorded::Held(release)
            }
            Reply::Answer(answer) => {
                state.commands.push(command);
                Recorded::Now(Ok(Box::pin(async move { Ok(answer) })))
            }
            Reply::Lost => {
                state.commands.push(command);
                Recorded::Now(Ok(Box::pin(async {
                    Err(Error::AnswerLost("closed".to_owned()))
                })))
            }
            Reply::Unreadable(text) => {
                state.commands.push(command);
                Recorded::Now(Ok(Box::pin(async move { Err(Error::Unreadable(text)) })))
            }
            Reply::Never => {
                state.commands.push(command);
                Recorded::Now(Ok(Box::pin(std::future::pending())))
            }
            Reply::Manual => {
                state.commands.push(command);
                drop(state);
                Recorded::Now(Ok(self.manual_wait(id)))
            }
        }
    }

    fn manual_wait(&self, command_id: Uuid) -> AnswerWait {
        let (sender, receiver) = oneshot::channel();
        self.state()
            .manual
            .entry(command_id)
            .or_default()
            .push(sender);
        Box::pin(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(Error::AnswerLost("dropped".to_owned())))
        })
    }

    pub fn respond(&self, command_id: Uuid, answer: Result<Answer, Error>) {
        let senders = self.state().manual.remove(&command_id).unwrap_or_default();
        for sender in senders {
            let _ = sender.send(answer.clone());
        }
    }
}

impl BusTransport for MemoryBus {
    fn ready(&self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            let mode = self.state().ready_mode.clone();
            match mode {
                ReadyMode::Open => Ok(()),
                ReadyMode::Unavailable(detail) => Err(Error::Unavailable(detail)),
                ReadyMode::Hold(release) => {
                    release.notified().await;
                    Ok(())
                }
            }
        })
    }

    fn send(&self, command: BusCommand) -> BoxFuture<'_, Result<AnswerWait, Error>> {
        Box::pin(async move {
            let id = command.command_id;
            match self.record(command) {
                Recorded::Held(release) => {
                    release.notified().await;
                    Ok(self.manual_wait(id))
                }
                Recorded::Now(result) => result,
            }
        })
    }

    fn publish_chunk(&self, chunk: OutboundChunk) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            let mode = {
                let mut state = self.state();
                state.chunks.push(chunk);
                state.chunk_mode.clone()
            };
            match mode {
                ChunkMode::Ack => Ok(()),
                ChunkMode::Fail => Err(Error::StreamBroken),
                ChunkMode::Hold(release) => {
                    release.notified().await;
                    Ok(())
                }
            }
        })
    }
}

#[derive(Clone, Default)]
pub struct MemoryGraphql {
    state: Arc<Mutex<GraphqlState>>,
}

#[derive(Default)]
struct GraphqlState {
    replies: VecDeque<Result<HttpReply, Error>>,
    bodies: Vec<Vec<u8>>,
}

impl MemoryGraphql {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, GraphqlState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn reply_with(&self, reply: Result<HttpReply, Error>) {
        self.state().replies.push_back(reply);
    }

    pub fn bodies(&self) -> Vec<Vec<u8>> {
        self.state().bodies.clone()
    }
}

impl GraphqlTransport for MemoryGraphql {
    fn post(&self, body: Vec<u8>) -> BoxFuture<'_, Result<HttpReply, Error>> {
        Box::pin(async move {
            let mut state = self.state();
            state.bodies.push(body);
            state
                .replies
                .pop_front()
                .unwrap_or_else(|| Err(Error::AnswerLost("no scripted reply".to_owned())))
        })
    }
}
