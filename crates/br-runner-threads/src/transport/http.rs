use br_llm_clients_common_auth::{TokenAuthorizer, TokenCache};
use br_llm_clients_common_http::{Endpoint, HttpClient, Request, Secret, SendError};

use super::{BoxFuture, GraphqlTransport, HttpReply};
use crate::error::Error;

#[derive(Clone)]
pub struct HttpGraphql {
    endpoint: Endpoint,
    client: HttpClient,
    authorizer: TokenAuthorizer,
    tokens: TokenCache,
}

impl HttpGraphql {
    pub fn new(endpoint: Endpoint, client: HttpClient, tokens: TokenCache) -> Self {
        Self {
            endpoint,
            client,
            authorizer: TokenAuthorizer::bearer(tokens.clone()),
            tokens,
        }
    }
}

fn scrubbed(text: String, secrets: &[Secret]) -> String {
    secrets.iter().fold(text, |text, secret| {
        text.replace(secret.expose(), "[redacted]")
    })
}

fn mapped(error: SendError, secrets: &[Secret]) -> Error {
    match error {
        SendError::Authorizer(cause) => Error::Auth(scrubbed(cause.to_string(), secrets)),
        SendError::InvalidAuthorization { text } => Error::Auth(scrubbed(text, secrets)),
        SendError::Connect { text }
        | SendError::Tls { text }
        | SendError::ConnectTimeout { text } => Error::Unavailable(scrubbed(text, secrets)),
        other => Error::AnswerLost(scrubbed(other.to_string(), secrets)),
    }
}

impl GraphqlTransport for HttpGraphql {
    fn post(&self, body: Vec<u8>) -> BoxFuture<'_, Result<HttpReply, Error>> {
        Box::pin(async move {
            let request = Request::new(self.endpoint.clone(), body)
                .with_header("content-type", "application/json")
                .map_err(|e| Error::invalid(e.to_string()))?;
            let mut response = self
                .client
                .send(request, &self.authorizer)
                .await
                .map_err(|e| mapped(e, &self.tokens.secrets()))?;
            let status = response.status();
            let body = match response.body_mut().collect(usize::MAX).await {
                Ok(collected) => collected.bytes,
                Err(_) if status == 401 || status == 403 => Vec::new(),
                Err(error) => {
                    return Err(Error::AnswerLost(scrubbed(
                        error.to_string(),
                        &self.tokens.secrets(),
                    )));
                }
            };
            Ok(HttpReply { status, body })
        })
    }
}
