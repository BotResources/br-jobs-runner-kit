mod check;
mod publish;

use async_nats::jetstream;
use br_runner_connection::Connection;
use br_util_nats_fabric::{ConnectionState, Fabric};

#[derive(Clone)]
pub(crate) struct Wire {
    fabric: Fabric,
    jetstream: jetstream::Context,
}

impl Wire {
    pub(crate) fn of(connection: &Connection) -> Self {
        let jetstream = jetstream::new(connection.bus().clone());
        Self {
            fabric: Fabric::new(jetstream.clone()),
            jetstream,
        }
    }

    pub(crate) fn fabric(&self) -> &Fabric {
        &self.fabric
    }

    pub(crate) fn connected(&self) -> bool {
        self.fabric.connection_state() == ConnectionState::Connected
    }
}
