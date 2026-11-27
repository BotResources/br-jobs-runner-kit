use std::process::ExitCode;

use battery::adapter::{self, Tunables};
use battery::env::Env;
use battery::error::{Res, data};
use br_agentic_harness_contract::{AgentId, Seed, ThreadRef, TurnId, TurnRef};
use uuid::Uuid;

fn uuid_of(text: &str) -> Res<Uuid> {
    Uuid::parse_str(text).map_err(|error| data("uuid argument", error.to_string()))
}

async fn run() -> Res {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 7 {
        return Err(data(
            "arguments",
            "thread agent turn parent store_endpoint context bytes",
        ));
    }
    let thread =
        ThreadRef::from_uuid(uuid_of(&args[0])?).map_err(|e| data("thread", e.to_string()))?;
    let agent = AgentId::from_uuid(uuid_of(&args[1])?).map_err(|e| data("agent", e.to_string()))?;
    let turn = TurnId::from_uuid(uuid_of(&args[2])?).map_err(|e| data("turn", e.to_string()))?;
    let parent = if args[3] == "-" {
        None
    } else {
        Some(TurnId::from_uuid(uuid_of(&args[3])?).map_err(|e| data("parent", e.to_string()))?)
    };
    let bytes: u64 = args[6].parse().map_err(|_| data("bytes", "not a number"))?;
    let env = Env::load()?;
    let mut tunables = Tunables::standard(&env);
    tunables.store_endpoint = args[4].clone();
    tunables.context = args[5].clone();
    let sandbox = tunables.connect(&env).await?;
    let opened = adapter::open(
        &sandbox,
        TurnRef::new(thread, agent, turn, parent),
        Seed::empty(),
    )
    .await?;
    battery::tree::state(&sandbox, &opened.box_id, 77, bytes).await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
