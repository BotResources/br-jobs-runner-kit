use br_runner_jobs::JobsClient;
use br_runner_toolkit::{HostEvent, Settings, StartArgs, TestHost};

#[path = "shared/greeter.rs"]
#[allow(dead_code)]
mod greeter;

pub async fn events_of_one_run(
    client: JobsClient,
    settings: br_runner_toolkit::SettingsBuilder,
) -> Result<Vec<HostEvent>, Box<dyn std::error::Error + Send + Sync>> {
    let args = StartArgs::new(greeter::graph(None)?, greeter::starter(), false);
    let host = TestHost::new(client, settings, args)?;
    Ok(host.run().await?.events)
}

fn main() {
    let _ = Settings::builder();
    println!("run this example through its test");
}
