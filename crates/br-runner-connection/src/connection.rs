use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use bytes::Bytes;
use http::HeaderValue;
use serde_json::{Value, json};

use crate::bus::{self, Client};
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
        if reply_wait.is_zero() {
            return Err(Error::InvalidInput("the reply wait is zero".to_owned()));
        }
        let endpoint = Endpoint::parse(endpoint)?;
        let secure = endpoint.secure;
        Ok(Self {
            inner: Arc::new(Inner {
                gateway: Gateway::new(endpoint, default_config()),
                secure,
                credential,
                bus,
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
        let body = graphql_body(document, variables)?;
        let token = self
            .inner
            .credential
            .token()
            .await
            .map_err(Error::Credential)?;
        let authorization = bearer(token.expose()).map_err(Error::Credential)?;
        drop(token);
        let reply = self
            .inner
            .gateway
            .post(authorization, body, self.inner.reply_wait)
            .await?;
        judge(reply)
    }

    pub async fn bus_request(
        &self,
        subject: &str,
        body: Bytes,
        wait: Duration,
    ) -> Result<Bytes, Error> {
        bus::request(&self.inner.bus, subject, body, wait).await
    }
}

fn graphql_body(document: &str, variables: Value) -> Result<Vec<u8>, Error> {
    if document.is_empty() {
        return Err(Error::InvalidInput("the document is empty".to_owned()));
    }
    let variables = match variables {
        Value::Null => json!({}),
        object @ Value::Object(_) => object,
        _ => {
            return Err(Error::InvalidInput(
                "the variables are not a JSON object".to_owned(),
            ));
        }
    };
    serde_json::to_vec(&json!({ "query": document, "variables": variables }))
        .map_err(|e| Error::InvalidInput(e.to_string()))
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

#[cfg(test)]
mod tests;
