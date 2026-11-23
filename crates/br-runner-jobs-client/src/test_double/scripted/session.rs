use std::time::Duration;

use tokio::sync::mpsc;

use super::{Answer, Call, ScriptedJobs};
use crate::backend::{BoxFuture, ChildCommand, ChildSession, RawFeed};
use crate::error::Error;

pub(in crate::test_double) struct Channel<T>(
    pub(in crate::test_double) mpsc::UnboundedReceiver<Result<T, Error>>,
);

impl<T: Send + 'static> RawFeed<T> for Channel<T> {
    fn next(&mut self) -> BoxFuture<'_, Result<T, Error>> {
        Box::pin(async move {
            self.0
                .recv()
                .await
                .unwrap_or_else(|| Err(Error::lost("the scripted feed was dropped")))
        })
    }
}

pub(in crate::test_double) fn answer<T>(
    slot: &mut Option<Answer<T>>,
    default: T,
) -> Result<T, Error> {
    match slot {
        Some(answer) => answer(),
        None => Ok(default),
    }
}

pub(in crate::test_double) struct ScriptedChild<'a> {
    pub(in crate::test_double) jobs: &'a ScriptedJobs,
    pub(in crate::test_double) command: ChildCommand,
}

impl ChildSession for ScriptedChild<'_> {
    fn attempt(&mut self, _wait: Duration) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.jobs.gate().await;
            let mut script = self.jobs.script();
            script.calls.push(Call::Command(self.command.clone()));
            answer(&mut script.command, ())
        })
    }
}
