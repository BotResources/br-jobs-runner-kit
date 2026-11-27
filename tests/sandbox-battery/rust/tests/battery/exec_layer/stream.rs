use std::time::Duration;

use battery::Session;
use battery::adapter::{self, Tunables};
use battery::error::{BatteryError, Res};
use battery::evidence::Checks;
use battery::guards::PodReaper;
use battery::ids::Lane;
use battery::tunnel::{GateMode, api_gate};
use br_agentic_harness_contract::{Command, RunMode, SandboxPort, Seed};

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_with_no_status_is_a_fault() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let (gate, context) = api_gate(env, "battery-exec-cut").await?;
    let mut tunables = Tunables::standard(env);
    tunables.context = context.name.clone();
    let sandbox = tunables.connect(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let pod = opened.box_id.to_string();
    let mut reaper = PodReaper::new(env);
    reaper.track(pod.clone());
    let runner = sandbox.clone();
    let boxed = opened.box_id.clone();
    let running = tokio::spawn(async move {
        let command = Command::new("sleep 120").map_err(|e| BatteryError::Port(e.to_string()))?;
        runner
            .run(&boxed, &command, RunMode::Foreground)
            .await
            .map_err(BatteryError::from)
    });
    tokio::time::sleep(Duration::from_secs(3)).await;
    gate.set(GateMode::Refuse).await?;
    let outcome = tokio::time::timeout(Duration::from_secs(60), running)
        .await
        .map_err(|_| BatteryError::Hang {
            what: "running a cut stream".to_owned(),
            seconds: 60,
        })?
        .map_err(|e| BatteryError::Port(e.to_string()))?;
    let text = format!("{outcome:?}");
    checks.expect(
        "exec-stream-without-status-is-never-an-exit-code",
        outcome.is_err(),
        text.clone(),
    );
    checks.expect(
        "exec-no-status-cause-is-visible-through-the-port",
        text.contains("NoStatus") || text.to_lowercase().contains("no status"),
        text,
    );
    gate.set(GateMode::Open).await?;
    checks.finish()
}
