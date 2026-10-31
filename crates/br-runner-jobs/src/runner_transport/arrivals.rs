use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use br_util_nats_fabric::{
    ConsumeErrorKind, Delivered, EventConsumer, EventCoords, Fabric, FabricError, INTEGRATION_EVT,
    IntegrationEvent,
};
use contract_jobs::event_v2::{ItemKind as WireKind, RunItemArrived};
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use uuid::Uuid;

use super::{Segments, Wire, fail};
use crate::backend::{BoxFuture, RawFeed};
use crate::error::Error;
use crate::model::{ItemArrived, ItemKind};

type Item = Result<ItemArrived, Error>;

enum Slot {
    Closed,
    Open(Box<EventConsumer<RunItemArrived>>),
    Failed(Error),
}

struct Route {
    run_id: Uuid,
    sender: mpsc::UnboundedSender<Item>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Consumer {
    Closed,
    Open,
    Failed,
}

struct Routes {
    next: u64,
    open: HashMap<u64, Route>,
    consumer: Consumer,
}

pub(super) struct Arrivals {
    fabric: Fabric,
    coords: EventCoords,
    durable: String,
    slot: AsyncMutex<Slot>,
    routes: Mutex<Routes>,
}

pub(super) struct ArrivalFeed {
    arrivals: Arc<Arrivals>,
    id: u64,
    receiver: mpsc::UnboundedReceiver<Item>,
}

fn kind_of(kind: WireKind) -> ItemKind {
    match kind {
        WireKind::Mail => ItemKind::DirectMail,
        WireKind::CcNotice => ItemKind::CcNotice,
        WireKind::Notice => ItemKind::CallNotice,
    }
}

fn consume_error(error: FabricError) -> Error {
    fail::fabric(&format!("the stream {INTEGRATION_EVT}"), error)
}

impl Arrivals {
    pub(super) fn new(wire: &Wire, segments: &Segments) -> Result<Self, Error> {
        let runner_type = segments.runner_type.as_str();
        let instance_key = segments.instance_key.as_str();
        Ok(Self {
            fabric: wire.fabric.clone(),
            coords: contract_jobs::evt_run_item_arrived_v1_coords()
                .map_err(|e| Error::invalid(e.to_string()))?,
            durable: format!(
                "runner_jobs_arrivals_{}_{runner_type}_{instance_key}",
                runner_type.len()
            ),
            slot: AsyncMutex::new(Slot::Closed),
            routes: Mutex::new(Routes {
                next: 0,
                open: HashMap::new(),
                consumer: Consumer::Closed,
            }),
        })
    }

    fn routes(&self) -> std::sync::MutexGuard<'_, Routes> {
        self.routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) async fn open(self: &Arc<Self>, run_id: Uuid) -> Result<ArrivalFeed, Error> {
        self.ensure_consumer().await?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut routes = self.routes();
        routes.next += 1;
        let id = routes.next;
        routes.open.insert(id, Route { run_id, sender });
        Ok(ArrivalFeed {
            arrivals: Arc::clone(self),
            id,
            receiver,
        })
    }

    async fn ensure_consumer(&self) -> Result<(), Error> {
        if self.routes().consumer == Consumer::Open {
            return Ok(());
        }
        let mut slot = self.slot.lock().await;
        if self.routes().consumer == Consumer::Open {
            return Ok(());
        }
        *slot = Slot::Closed;
        self.routes().consumer = Consumer::Closed;
        let consumer = self
            .fabric
            .ensure_event_consumer::<RunItemArrived>(&self.coords, &self.durable)
            .await
            .map_err(consume_error)?;
        *slot = Slot::Open(Box::new(consumer));
        self.routes().consumer = Consumer::Open;
        Ok(())
    }

    fn send_to_run(&self, run_id: Uuid, item: &Item) {
        for route in self.routes().open.values() {
            if route.run_id == run_id {
                let _ = route.sender.send(item.clone());
            }
        }
    }

    fn send_to_all(&self, item: &Item) {
        for route in self.routes().open.values() {
            let _ = route.sender.send(item.clone());
        }
    }

    async fn failure(&self, error: FabricError) -> Error {
        let gone = self
            .fabric
            .await_events(&[&self.coords])
            .await
            .map(drop)
            .err();
        match gone {
            Some(missing) => consume_error(missing),
            None => consume_error(error),
        }
    }

    async fn take(&self) -> Result<Option<Delivered<IntegrationEvent<RunItemArrived>>>, Error> {
        let mut slot = self.slot.lock().await;
        let consumer = match &mut *slot {
            Slot::Open(consumer) => consumer,
            Slot::Failed(error) => return Err(error.clone()),
            Slot::Closed => return Err(Error::lost("the arrival consumer is not open")),
        };
        let failure = match consumer.recv().await {
            Ok(Some(delivered)) => return Ok(Some(delivered)),
            Ok(None) => Error::lost("the arrival consumer ended"),
            Err(FabricError::Consume {
                kind: ConsumeErrorKind::HeartbeatMissed,
                ..
            }) => return Ok(None),
            Err(error) => self.failure(error).await,
        };
        *slot = Slot::Failed(failure.clone());
        self.routes().consumer = Consumer::Failed;
        Err(failure)
    }

    async fn pump(&self) -> Result<(), Error> {
        let Some(delivered) = self.take().await? else {
            return Ok(());
        };
        match delivered.payload() {
            Ok(event) => {
                let arrived = ItemArrived {
                    address: event.payload.address,
                    item_id: event.payload.item_id,
                    kind: kind_of(event.payload.item_kind),
                };
                let run_id = event.payload.run_id;
                let _ = delivered.ack().await;
                self.send_to_run(run_id, &Ok(arrived));
            }
            Err(e) => {
                let unreadable = Error::Unreadable(e.to_string());
                let _ = delivered.term().await;
                self.send_to_all(&Err(unreadable));
            }
        }
        Ok(())
    }
}

impl Drop for ArrivalFeed {
    fn drop(&mut self) {
        self.arrivals.routes().open.remove(&self.id);
    }
}

impl RawFeed<ItemArrived> for ArrivalFeed {
    fn next(&mut self) -> BoxFuture<'_, Result<ItemArrived, Error>> {
        Box::pin(async move {
            loop {
                tokio::select! {
                    biased;
                    item = self.receiver.recv() => {
                        return item.unwrap_or_else(|| Err(Error::lost("the arrival feed was closed")));
                    }
                    pumped = self.arrivals.pump() => pumped?,
                }
            }
        })
    }
}
