use std::sync::Arc;
use std::time::Duration;

use br_runner_threads::transport::{Answer, MemoryBus, Reply};
use br_runner_threads::{Closing, ClosureKind, Error, Refusal, StreamHandle};
use contract_threads::CompleteMessageV2;
use uuid::Uuid;

use super::*;

pub fn manual_closings(world: &World) {
    world.bus.answer_with(|c| {
        if c.aggregate == "message" && (c.verb == "complete" || c.verb == "fail") {
            Reply::Manual
        } else {
            confirmed(c.command_id, None)
        }
    });
}

pub async fn commands_sent(bus: &MemoryBus, count: usize) {
    for _ in 0..200 {
        if bus.commands().len() >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("{count} commands were not sent");
}

pub fn refusal(command_id: Uuid) -> Result<Answer, Error> {
    Ok(Answer::Refused(Refusal {
        code: "STREAM_MISMATCH".to_owned(),
        params: serde_json::Map::new(),
        command_id: Some(command_id),
    }))
}

pub struct Fixture {
    pub world: World,
    pub handle: Arc<StreamHandle>,
}

pub async fn fixture() -> Fixture {
    let world = world();
    let handle = Arc::new(open(&world).await);
    handle.keepalive().await.expect("chunk");
    manual_closings(&world);
    Fixture { world, handle }
}

impl Fixture {
    pub fn start(
        &self,
        kind: ClosureKind,
    ) -> (Closing, tokio::task::JoinHandle<Result<(), Error>>) {
        let closing = closing(&self.handle, self.world.thread, kind);
        let task = {
            let (client, handle, closing) = (
                self.world.client.clone(),
                Arc::clone(&self.handle),
                closing.clone(),
            );
            tokio::spawn(async move {
                client
                    .close_message(&closing, Some(&handle))
                    .await
                    .map(drop)
            })
        };
        (closing, task)
    }

    pub async fn close(&self, kind: ClosureKind) -> Result<(), Error> {
        let closing = closing(&self.handle, self.world.thread, kind);
        self.world
            .client
            .close_message(&closing, Some(&self.handle))
            .await
            .map(drop)
    }
}

pub fn seals_sent(world: &World) -> Vec<contract_threads::StreamSeal> {
    world
        .bus
        .commands()
        .iter()
        .filter(|c| c.verb == "complete")
        .map(|c| {
            serde_json::from_value::<CompleteMessageV2>(c.payload.clone())
                .expect("wire")
                .seal
        })
        .collect()
}
