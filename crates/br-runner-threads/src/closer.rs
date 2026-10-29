use crate::client::{ThreadsClient, answered};
use crate::closing::Closing;
use crate::commands;
use crate::error::Error;
use crate::stream::StreamHandle;
use crate::transport::Confirmed;

pub(crate) async fn close(
    client: &ThreadsClient,
    closing: &Closing,
    handle: Option<&StreamHandle>,
) -> Result<Confirmed, Error> {
    closing.kind.reason(closing.detail.as_deref()).map(drop)?;
    let Some(handle) = handle else {
        if commands::needs_handle(closing.kind) {
            return Err(Error::invalid(
                "a Completed or BudgetExhausted closing needs the handle of the stream",
            ));
        }
        let command = commands::closing(client.actor, closing, None)?;
        let pending = client.bus.send(command).await?;
        return answered(pending, closing.wait).await;
    };
    if handle.message_id() != closing.message_id {
        return Err(Error::invalid("the handle belongs to another message"));
    }
    let core = &handle.core;
    let chunks = core.gate.lock().await;
    let sealed = closing.kind.is_sealed() && !chunks.is_empty();
    if sealed {
        core.judge_sealed_closing()?;
    } else {
        core.judge_unsealed_closing()?;
    }
    let seal = if sealed {
        Some(commands::seal_of(&chunks)?)
    } else {
        None
    };
    let command = commands::closing(client.actor, closing, seal)?;
    let guard = core.begin_closing();
    let pending = match core.bus.send(command).await {
        Ok(pending) => pending,
        Err(Error::Unavailable(detail)) => {
            guard.unsent();
            return Err(Error::Unavailable(detail));
        }
        Err(other) => {
            guard.lost();
            return Err(other);
        }
    };
    drop(chunks);
    match answered(pending, closing.wait).await {
        Ok(confirmed) => {
            guard.confirmed();
            Ok(confirmed)
        }
        Err(Error::Refused(refusal)) => {
            guard.refused();
            Err(Error::Refused(refusal))
        }
        Err(other) => {
            guard.lost();
            Err(other)
        }
    }
}
