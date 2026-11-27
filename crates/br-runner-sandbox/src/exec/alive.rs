use bytes::Bytes;

use crate::cluster::{ClusterError, ExecIo, PodView};
use crate::env::Env;
use crate::error::Error;

use super::session::{ExecOutput, collect, expect_success};

const SIGNAL_EXIT_FLOOR: i32 = 128;
const ENDED_PHASES: [&str; 2] = ["Failed", "Succeeded"];

fn ended(view: &PodView) -> bool {
    view.phase
        .as_deref()
        .is_some_and(|phase| ENDED_PHASES.contains(&phase))
}

impl Env {
    pub(crate) async fn collect_in_pod(
        &self,
        operation: &'static str,
        pod: &str,
        io: ExecIo,
        input: Option<Bytes>,
    ) -> Result<ExecOutput, Error> {
        let output = collect(operation, io, input).await?;
        self.confirm_pod_after(pod, output.code).await?;
        Ok(output)
    }

    pub(crate) async fn succeed_in_pod(
        &self,
        operation: &'static str,
        pod: &str,
        io: ExecIo,
        input: Option<Bytes>,
    ) -> Result<ExecOutput, Error> {
        let output = self.collect_in_pod(operation, pod, io, input).await?;
        expect_success(operation, output.code)?;
        Ok(output)
    }

    async fn confirm_pod_after(&self, pod: &str, code: i32) -> Result<(), Error> {
        if code < SIGNAL_EXIT_FLOOR {
            return Ok(());
        }
        match self.cluster.get_pod(pod).await? {
            Some(view) if !view.deleting && !ended(&view) => Ok(()),
            _ => Err(ClusterError::Gone(pod.to_owned()).into()),
        }
    }
}
