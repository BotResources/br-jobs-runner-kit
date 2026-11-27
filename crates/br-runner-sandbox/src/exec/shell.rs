use br_agentic_harness_contract::{
    Command, CommandResult, FilePath, ProcessId, RunMode, RunOutcome,
};
use bytes::Bytes;

use crate::env::Env;
use crate::error::Error;

use super::argv;
use super::error::ExecError;
use super::list::parse_find_output;
use super::session::bounded;
use super::write_archive::file_archive;

impl Env {
    pub async fn run(
        &self,
        pod: &str,
        command: &Command,
        mode: RunMode,
    ) -> Result<RunOutcome, Error> {
        match mode {
            RunMode::Foreground => self.run_foreground(pod, command).await,
            RunMode::Background => self.run_background(pod, command).await,
        }
    }

    async fn run_foreground(&self, pod: &str, command: &Command) -> Result<RunOutcome, Error> {
        let request = self.agent_request(pod, argv::run_foreground(command.as_str()), false);
        let io = self.cluster.exec(request).await?;
        let output = bounded::<_, Error>(
            "run",
            self.config.exec_timeout(),
            self.collect_in_pod("run", pod, io, None),
        )
        .await?;
        Ok(RunOutcome::Finished(CommandResult {
            exit_code: output.code,
            stdout: output.stdout,
            stderr: output.stderr,
        }))
    }

    async fn run_background(&self, pod: &str, command: &Command) -> Result<RunOutcome, Error> {
        let request = self.agent_request(pod, argv::run_background(command.as_str()), false);
        let io = self.cluster.exec(request).await?;
        let output = self.succeed_in_pod("run", pod, io, None).await?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        match text.parse::<u32>() {
            Ok(process) if process > 0 => {
                Ok(RunOutcome::Started(ProcessId::new(process.to_string())))
            }
            _ => Err(ExecError::Output { operation: "run" }.into()),
        }
    }

    pub async fn read(&self, pod: &str, path: &FilePath) -> Result<Vec<u8>, Error> {
        let io = self
            .cluster
            .exec(self.agent_request(pod, argv::read(path), false))
            .await?;
        let output = self.collect_in_pod("read", pod, io, None).await?;
        if output.code == 0 {
            Ok(output.stdout)
        } else {
            Err(ExecError::Unreadable {
                path: path.to_string(),
            }
            .into())
        }
    }

    pub async fn write(&self, pod: &str, path: &FilePath, content: &[u8]) -> Result<(), Error> {
        let archive = file_archive(path, content, self.clock.now())?;
        let request = self.agent_request(pod, argv::tar_write(self.config.tools_path()), true);
        let io = self.cluster.exec(request).await?;
        self.succeed_in_pod("write", pod, io, Some(Bytes::from(archive)))
            .await?;
        Ok(())
    }

    pub async fn list(&self, pod: &str, under: Option<&FilePath>) -> Result<Vec<FilePath>, Error> {
        let io = self
            .cluster
            .exec(self.agent_request(pod, argv::list(under), false))
            .await?;
        let output = self.collect_in_pod("list", pod, io, None).await?;
        parse_find_output(output.code, &output.stdout).map_err(Error::from)
    }

    pub async fn kill(&self, pod: &str, process: &ProcessId) -> Result<(), Error> {
        let number = process
            .as_str()
            .parse::<u32>()
            .ok()
            .filter(|number| *number > 0 && number.to_string() == process.as_str())
            .ok_or_else(|| ExecError::ProcessId {
                text: process.to_string(),
            })?;
        let io = self
            .cluster
            .exec(self.agent_request(pod, argv::kill(number), false))
            .await?;
        self.succeed_in_pod("kill", pod, io, None).await?;
        Ok(())
    }
}
