use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecRequest {
    pub pod: String,
    pub container: String,
    pub command: Vec<String>,
    pub stdin: bool,
}

pub type ExecStdin = Box<dyn AsyncWrite + Send + Unpin>;
pub type ExecOut = Box<dyn AsyncRead + Send + Unpin>;

pub struct ExecIo {
    pub stdin: Option<ExecStdin>,
    pub stdout: ExecOut,
    pub stderr: ExecOut,
    pub status: BoxFuture<'static, Option<i32>>,
}
