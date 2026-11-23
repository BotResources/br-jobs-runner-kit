use async_nats::jetstream::Context;
use br_core_kernel::Actor;
use br_util_nats_fabric::{EventCoords, EventMetadata, Fabric, IntegrationEvent};
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::live_world::account;

pub async fn try_publish_event<T: Serialize>(
    js: &Context,
    coords: &EventCoords,
    correlation_id: Uuid,
    payload: T,
) -> Result<(), String> {
    let metadata = EventMetadata::new(Actor::Service(account()), correlation_id);
    let event = IntegrationEvent::new(Uuid::now_v7(), "stand-in", 1, Utc::now(), metadata, payload);
    Fabric::new(js.clone())
        .publish_event(coords, &event)
        .await
        .map(drop)
        .map_err(|error| error.to_string())
}

pub async fn publish_event<T: Serialize>(
    js: &Context,
    coords: &EventCoords,
    correlation_id: Uuid,
    payload: T,
) {
    try_publish_event(js, coords, correlation_id, payload)
        .await
        .expect("the event is published");
}
