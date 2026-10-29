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
}

#[derive(Clone)]
pub enum ChunkMode {
    Ack,
    Fail,
    Hold(Arc<Notify>),
}

type Script = Box<dyn Fn(&BusCommand) -> Reply + Send + Sync>;

struct State {
    commands: Vec<BusCommand>,
    chunks: Vec<OutboundChunk>,
    script: Script,
    chunk_mode: ChunkMode,
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

    pub fn commands(&self) -> Vec<BusCommand> {
        self.state().commands.clone()
    }

    pub fn chunks(&self) -> Vec<OutboundChunk> {
        self.state().chunks.clone()
    }

    pub fn respond(&self, command_id: Uuid, answer: Result<Answer, Error>) {
        let senders = self.state().manual.remove(&command_id).unwrap_or_default();
        for sender in senders {
            let _ = sender.send(answer.clone());
        }
    }
}

impl BusTransport for MemoryBus {
    fn send(&self, command: BusCommand) -> BoxFuture<'_, Result<AnswerWait, Error>> {
        Box::pin(async move {
            let mut state = self.state();
            let reply = (state.script)(&command);
            let id = command.command_id;
            match reply {
                Reply::Unavailable(detail) => return Err(Error::Unavailable(detail)),
                Reply::PublishLost(detail) => {
                    state.commands.push(command);
                    return Err(Error::AnswerLost(detail));
                }
                _ => state.commands.push(command),
            }
            let wait: AnswerWait = match reply {
                Reply::Answer(answer) => Box::pin(async move { Ok(answer) }),
                Reply::Lost => Box::pin(async { Err(Error::AnswerLost("closed".to_owned())) }),
                Reply::Unreadable(text) => Box::pin(async move { Err(Error::Unreadable(text)) }),
                Reply::Never => Box::pin(std::future::pending()),
                _ => {
                    let (sender, receiver) = oneshot::channel();
                    state.manual.entry(id).or_default().push(sender);
                    Box::pin(async move {
                        receiver
                            .await
                            .unwrap_or_else(|_| Err(Error::AnswerLost("dropped".to_owned())))
                    })
                }
            };
            Ok(wait)
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
