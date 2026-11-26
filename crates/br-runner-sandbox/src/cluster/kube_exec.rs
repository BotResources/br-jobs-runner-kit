use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::AttachParams;

use super::error::ClusterError;
use super::exec::{ExecIo, ExecOut, ExecRequest, ExecStdin};
use super::kube_convert::{call_error, exit_code};

fn missing(stream: &str) -> ClusterError {
    ClusterError::Call(format!("the exec session has no {stream} stream"))
}

pub(super) async fn start(api: &Api<Pod>, request: ExecRequest) -> Result<ExecIo, ClusterError> {
    let params = AttachParams::default()
        .container(request.container.clone())
        .stdin(request.stdin)
        .stdout(true)
        .stderr(true)
        .tty(false);
    let mut process = api
        .exec(&request.pod, request.command.clone(), &params)
        .await
        .map_err(|error| call_error(&request.pod, error))?;
    let stdin = process.stdin().map(|writer| Box::new(writer) as ExecStdin);
    let stdout = process
        .stdout()
        .map(|reader| Box::new(reader) as ExecOut)
        .ok_or_else(|| missing("stdout"))?;
    let stderr = process
        .stderr()
        .map(|reader| Box::new(reader) as ExecOut)
        .ok_or_else(|| missing("stderr"))?;
    let status = process.take_status().ok_or_else(|| missing("status"))?;
    Ok(ExecIo {
        stdin,
        stdout,
        stderr,
        status: Box::pin(async move {
            let code = status.await.as_ref().and_then(exit_code);
            let _ = process.join().await;
            code
        }),
    })
}
