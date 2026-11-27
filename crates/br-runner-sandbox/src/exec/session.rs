use std::future::Future;
use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::cluster::{ExecIo, ExecOut, ExecStdin};

use super::error::ExecError;

pub const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub async fn read_all(operation: &'static str, mut reader: ExecOut) -> Result<Vec<u8>, ExecError> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| ExecError::io(operation, error))?;
    Ok(bytes)
}

pub async fn drain(operation: &'static str, mut reader: ExecOut) -> Result<(), ExecError> {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) => return Err(ExecError::io(operation, error)),
        }
    }
}

pub async fn close(operation: &'static str, mut stdin: ExecStdin) -> Result<(), ExecError> {
    stdin
        .shutdown()
        .await
        .map_err(|error| ExecError::io(operation, error))
}

pub async fn exit_code(
    operation: &'static str,
    status: impl Future<Output = Option<i32>>,
) -> Result<i32, ExecError> {
    status.await.ok_or(ExecError::NoStatus { operation })
}

pub fn expect_success(operation: &'static str, code: i32) -> Result<(), ExecError> {
    if code == 0 {
        Ok(())
    } else {
        Err(ExecError::Failed { operation, code })
    }
}

async fn feed(
    operation: &'static str,
    stdin: Option<ExecStdin>,
    input: Option<Bytes>,
) -> Result<(), ExecError> {
    let Some(mut stdin) = stdin else {
        return match input {
            Some(_) => Err(ExecError::Output { operation }),
            None => Ok(()),
        };
    };
    if let Some(input) = input {
        stdin
            .write_all(&input)
            .await
            .map_err(|error| ExecError::io(operation, error))?;
    }
    close(operation, stdin).await
}

pub async fn collect(
    operation: &'static str,
    io: ExecIo,
    input: Option<Bytes>,
) -> Result<ExecOutput, ExecError> {
    let ExecIo {
        stdin,
        stdout,
        stderr,
        status,
    } = io;
    let ((), stdout, stderr, code) = tokio::try_join!(
        feed(operation, stdin, input),
        read_all(operation, stdout),
        read_all(operation, stderr),
        exit_code(operation, status)
    )?;
    Ok(ExecOutput {
        code,
        stdout,
        stderr,
    })
}

pub async fn succeed(
    operation: &'static str,
    io: ExecIo,
    input: Option<Bytes>,
) -> Result<ExecOutput, ExecError> {
    let output = collect(operation, io, input).await?;
    expect_success(operation, output.code)?;
    Ok(output)
}

pub async fn bounded<T, E: From<ExecError>>(
    operation: &'static str,
    limit: Option<Duration>,
    future: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    match limit {
        None => future.await,
        Some(limit) => tokio::time::timeout(limit, future)
            .await
            .unwrap_or_else(|_| Err(E::from(ExecError::TimedOut { operation }))),
    }
}
