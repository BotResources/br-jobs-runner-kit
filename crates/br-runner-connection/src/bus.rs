use std::sync::atomic::Ordering;
use std::time::Duration;

use async_nats::{Client, Request, RequestErrorKind};
use br_util_nats_fabric::{ConnectionState, Fabric};
use bytes::Bytes;

use crate::error::{Error, Failure};

pub(crate) async fn request(
    fabric: &Fabric,
    client: &Client,
    subject: &str,
    body: Bytes,
    wait: Duration,
) -> Result<Bytes, Error> {
    if wait.is_zero() {
        return Err(Error::InvalidInput("the bus wait is zero".to_owned()));
    }
    if fabric.connection_state() != ConnectionState::Connected {
        return Err(Error::Unavailable(Failure::transport(
            "the bus connection is not connected",
        )));
    }
    let connects = client.statistics().connects.load(Ordering::SeqCst);
    let request = Request::new().payload(body).timeout(Some(wait));
    match client.send_request(subject.to_owned(), request).await {
        Ok(message) => Ok(message.payload),
        Err(error) => Err(match error.kind() {
            RequestErrorKind::InvalidSubject => Error::InvalidInput(error.to_string()),
            RequestErrorKind::NoResponders => {
                Error::Unavailable(Failure::transport("no service listens to the subject"))
            }
            RequestErrorKind::TimedOut => {
                let cut = fabric.connection_state() != ConnectionState::Connected
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
