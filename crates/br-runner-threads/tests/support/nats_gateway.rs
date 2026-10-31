use std::time::Duration;

use br_runner_threads::ThreadClient;

use super::nats_fixtures::{CHECK_WAIT, connection_with_reply_wait};
use super::nats_http::{Answer, HttpStandIn, Recorded};
use super::nats_server::Server;
use super::nats_streams::create_all_streams;

pub struct GatewayRig {
    _server: Server,
    pub gateway: HttpStandIn,
    pub client: ThreadClient,
}

pub async fn gateway_rig_with(
    reply_wait: Duration,
    handler: impl Fn(&Recorded) -> Answer + Send + Sync + 'static,
) -> GatewayRig {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;
    let gateway = HttpStandIn::start(handler).await;
    let connection = connection_with_reply_wait(&bus, &gateway.url(), reply_wait);
    let client = ThreadClient::new(&connection, CHECK_WAIT)
        .await
        .expect("the client is built");
    GatewayRig {
        _server: server,
        gateway,
        client,
    }
}

pub async fn gateway_rig(
    handler: impl Fn(&Recorded) -> Answer + Send + Sync + 'static,
) -> GatewayRig {
    gateway_rig_with(Duration::from_secs(5), handler).await
}
