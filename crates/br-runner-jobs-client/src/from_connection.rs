use br_runner_connection as connection;

use crate::error::{Cause, CredentialFailure, CredentialKind, Error, Failure, Operation, Refusal};

fn cause(cause: connection::Cause) -> Cause {
    match cause {
        connection::Cause::WaitFired => Cause::WaitFired,
        connection::Cause::NoListener => Cause::NoListener,
        _ => Cause::Transport,
    }
}

fn failure(failure: connection::Failure) -> Failure {
    Failure {
        cause: cause(failure.cause),
        text: failure.text,
    }
}

fn credential(failure: connection::CredentialFailure) -> CredentialFailure {
    CredentialFailure {
        kind: match failure.kind {
            connection::CredentialKind::NoUsable => CredentialKind::NoUsable,
            _ => CredentialKind::FetchFailed,
        },
        text: failure.text,
    }
}

pub(crate) fn error(error: connection::Error, operation: Operation) -> Error {
    match error {
        connection::Error::InvalidInput(text) => Error::InvalidInput(text),
        connection::Error::Credential(value) => Error::Credential(credential(value)),
        connection::Error::Unavailable(value) => Error::Unavailable(failure(value)),
        connection::Error::Lost(value) => Error::Lost(failure(value)),
        connection::Error::Unauthenticated => Error::Unauthenticated,
        connection::Error::Refused(refusal) => Error::Refused(Refusal::new(
            operation,
            refusal.reason.unwrap_or(refusal.code),
            refusal.message,
        )),
        connection::Error::Unreadable(text) => Error::Unreadable(text),
        other => Error::Lost(Failure::transport(other.to_string())),
    }
}
