use std::sync::atomic::Ordering;
use std::time::Duration;

use async_nats::connection::State;
use async_nats::{Request, RequestErrorKind};
use bytes::Bytes;

use super::Client;
use super::subject;
use crate::error::{Error, Failure};

fn connected(client: &Client) -> bool {
    client.connection_state() == State::Connected
}

pub(crate) async fn request(
    client: &Client,
    subject: &str,
    body: Bytes,
    wait: Duration,
) -> Result<Bytes, Error> {
    if wait.is_zero() {
        return Err(Error::InvalidInput("the bus wait is zero".to_owned()));
    }
    subject::check(subject)?;
    if !connected(client) {
        return Err(Error::Unavailable(Failure::transport(
            "the bus connection is not connected",
        )));
    }
    let limit = client.max_payload();
    if body.len() > limit {
        return Err(Error::InvalidInput(format!(
            "the bus body of {} bytes is over the limit of {limit} bytes of the bus",
            body.len()
        )));
    }
    let connects = client.statistics().connects.load(Ordering::SeqCst);
    let request = Request::new().payload(body).timeout(Some(wait));
    match client.send_request(subject.to_owned(), request).await {
        Ok(message) => Ok(message.payload),
        Err(error) => Err(match error.kind() {
            RequestErrorKind::InvalidSubject => Error::InvalidInput(error.to_string()),
            RequestErrorKind::NoResponders => {
                Error::Unavailable(Failure::no_listener("no service listens to the subject"))
            }
            RequestErrorKind::TimedOut => {
                let cut = !connected(client)
                    || client.statistics().connects.load(Ordering::SeqCst) != connects;
                Error::Lost(if cut {
                    Failure::transport("the bus session was cut before the reply")
                } else {
                    Failure::wait("no reply within the wait")
                })
            }
            _ => Error::Lost(Failure::transport(error.to_string())),
        }),
    }
}
