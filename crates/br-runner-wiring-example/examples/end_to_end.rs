use std::time::Duration;

use br_runner_wiring_example::{Variant, World, run_in_host};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for variant in [Variant::Harness, Variant::Gate] {
        let world = World::new(variant, Duration::from_secs(5))?;
        let run = run_in_host(&world).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        println!("{variant:?}: {} host events", run.events.len());
        println!("{variant:?}: node outcomes {:?}", world.observed.outcomes());
        println!(
            "{variant:?}: thread commands {}",
            world.threads.commands().len()
        );
    }
    Ok(())
}
