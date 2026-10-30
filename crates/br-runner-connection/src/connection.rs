use std::sync::Arc;
use std::time::Duration;

use async_nats::Client;
use br_core_kernel::ServiceAccountId;
use br_util_nats_fabric::Fabric;
use bytes::Bytes;
use http::HeaderValue;
use serde_json::{Value, json};

use crate::bus;
use crate::credential::{Credential, CredentialFailure};
use crate::endpoint::Endpoint;
use crate::error::Error;
use crate::http::{Gateway, default_config};
use crate::reply::judge;

struct Inner {
    gateway: Gateway,
    secure: bool,
    credential: Arc<dyn Credential>,
    bus: Client,
    fabric: Fabric,
    service_account: ServiceAccountId,
    reply_wait: Duration,
}

#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("service_account", &self.inner.service_account)
            .field("secure", &self.inner.secure)
            .field("reply_wait", &self.inner.reply_wait)
            .finish_non_exhaustive()
    }
}

impl Connection {
    pub fn new(
        endpoint: &str,
        credential: Arc<dyn Credential>,
        bus: Client,
        service_account: ServiceAccountId,
        reply_wait: Duration,
    ) -> Result<Self, Error> {
        let endpoint = Endpoint::parse(endpoint)?;
        let secure = endpoint.secure;
        let fabric = Fabric::new(async_nats::jetstream::new(bus.clone()));
        Ok(Self {
            inner: Arc::new(Inner {
                gateway: Gateway::new(endpoint, default_config()),
                secure,
                credential,
                bus,
                fabric,
                service_account,
                reply_wait,
            }),
        })
    }

    pub fn bus(&self) -> &Client {
        &self.inner.bus
    }

    pub fn service_account(&self) -> ServiceAccountId {
        self.inner.service_account
    }

    pub async fn graphql(&self, document: &str, variables: Value) -> Result<Value, Error> {
        let token = self
            .inner
            .credential
            .token()
            .await
            .map_err(Error::Credential)?;
        let secret = token.expose();
        let authorization = bearer(secret).map_err(Error::Credential)?;
        let body = serde_json::to_vec(&json!({ "query": document, "variables": variables }))
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        let outcome = match self
            .inner
            .gateway
            .post(authorization, body, self.inner.reply_wait)
            .await
        {
            Ok(reply) => judge(reply),
            Err(error) => Err(error),
        };
        outcome.map_err(|error| error.scrubbed(secret))
    }

    pub async fn bus_request(
        &self,
        subject: &str,
        body: Bytes,
        wait: Duration,
    ) -> Result<Bytes, Error> {
        bus::request(&self.inner.fabric, &self.inner.bus, subject, body, wait).await
    }
}

fn bearer(secret: &str) -> Result<HeaderValue, CredentialFailure> {
    if secret.is_empty() {
        return Err(CredentialFailure::no_usable(
            "the credential gave an empty token",
        ));
    }
    let mut value = HeaderValue::try_from(format!("Bearer {secret}")).map_err(|_| {
        CredentialFailure::no_usable("the credential gave a token that is not a header value")
    })?;
    value.set_sensitive(true);
    Ok(value)
}
