# br-runner-connection

The one connection of a runner to the platform. Runner code builds one `Connection` and gives it to `br-runner-threads` and `br-runner-jobs`. It does the bearer-authenticated GraphQL request, maps every failure to a small error vocabulary, keeps every token out of every text that it writes, gives the bus connection to the runner libraries and offers one request with one reply on the bus. It knows no thread, job, run, mail, agent or Passport, and it sends each request once.

## Install

```toml
br-runner-connection = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-connection", tag = "br-runner-connection/v0.1.1", version = "0.1.1" }
```

## Quickstart

This is `examples/build_connection.rs`, built by `cargo test` and checked word for word against this page by a test.

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, Error, FixedToken};

pub fn build_connection(
    endpoint: &str,
    token: &str,
    bus: async_nats::Client,
    service_account: ServiceAccountId,
) -> Result<Connection, Error> {
    Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token)),
        bus,
        service_account,
        Duration::from_secs(30),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, endpoint, nats_url, token, account] = args.as_slice() else {
        return Err(
            "usage: build_connection <endpoint> <nats-url> <token> <service-account-uuid>".into(),
        );
    };
    let bus = async_nats::connect(nats_url).await?;
    let service_account = ServiceAccountId(account.parse()?);
    let connection = build_connection(endpoint, token, bus, service_account)?;
    let data = connection
        .graphql("query { me { id } }", serde_json::json!({}))
        .await?;
    println!("{data}");
    Ok(())
}
```

The other examples are in `examples/` and run in the tests against a real local HTTP endpoint and a real bus: `graphql_call` (each error kind matched, `Refused` read by its code), `custom_credential` (your own `Credential`, and its two failure kinds), `one_connection_for_all` (one `Connection` used by two runner libraries at the same time).

## Surface

| Item | Role |
|---|---|
| `Connection::new(endpoint, credential, bus, service_account, reply_wait)` | Builds the connection. Every value is required. Sends nothing, binds nothing, runs no task. `Clone`. |
| `Connection::graphql(document, variables)` | Posts the document once, with the bearer token, and gives the `data` as JSON. |
| `Connection::bus_request(subject, body, wait)` | One request on core NATS, one reply. Gives the body of the reply. |
| `Connection::bus`, `Connection::service_account` | Give back what the connection was built with. |
| `Credential` | The seam: gives one bearer token, asked before each GraphQL request and only then. |
| `FixedToken` | A credential that holds one fixed secret token. |
| `Error`, `Failure`, `Cause`, `Refusal`, `CodeOrigin`, `CredentialFailure`, `CredentialKind` | The single error type and its parts. All are `#[non_exhaustive]`: match with a final `_` arm. |

## Bus access

The bus connection is an `async_nats::Client`. The importer builds and authenticates it with the bus credentials of the runner; `Connection::bus()` gives it back and `Connection::bus_request` runs on it. `async_nats` is named in one module only, `src/bus/`. The request is generic: the caller gives the subject and the body, and the library has no subject, stream or bucket name of its own. Subject names must come from the contract crate of the service that owns the wire, in the runner library that calls. The published language and `integration.*` must go through `br-util-nats-fabric`, never through this request. This request is meant to be called only by the `runner_transport` module of `br-runner-jobs` and the `chunk_lane` module of `br-runner-threads`, when they take a `Connection`; neither library calls this crate yet. The direct use of `async-nats` here rests on the 2026-08-14 runner-transport exception, which covers the `svc-jobs` runner channels; its extension to this crate is pending ratification. The library creates and binds no stream, no bucket and no consumer.

## Rules the code keeps

- The endpoint is an absolute `http` or `https` URL with no user information and no fragment, whose port, when it has one, is a number from 1 to 65535 (leading zeros are read as the number; a missing port is 80 for `http` and 443 for `https`, never the fallback of a bad one), and whose bracketed host, when it has one, is an IPv6 address with nothing but a port after the bracket, else `InvalidInput` at building; a query without a path is sent as `/?query`; the refusal text does not repeat the URL. The library reads no environment variable and no file. The HTTP client uses no system proxy and no cookie store and follows no redirect (a 3xx without a readable code is `Lost`). A zero reply wait is `InvalidInput` at building.
- With an `http://` endpoint the bearer token travels in clear to the host the importer gave, whatever that host is. The library does not refuse `http` for any host.
- With `https://` the server certificate is checked against the webpki roots only (no system store, no private authority, no way to add one) and against the host of the endpoint; a failure is `Unavailable` with the transport as cause.
- One new TCP connection and, for `https`, one TLS handshake per call; HTTP/1.1, no pool, no task. The reply wait counts from the moment the token is in hand and covers the connection, the request and the whole answer. Getting the token is the work of the credential and is not counted or bounded. A wait too long to be added to the clock (`Duration::MAX`) never fires. The library sets no bound on the size of a response; the wait bounds the time to read it.
- `graphql` checks its input before it asks the credential: an empty document, or variables that are neither a JSON object nor `null`, are `InvalidInput` and nothing is sent. `null` variables are sent as `{}`.
- Before sending (connect failure, failed handshake, wait fired before the connection was ready) is `Unavailable`. After the connection was ready, an interrupted request, a protocol error or the wait firing is `Lost`. Both carry a `Cause`: `WaitFired`, `Transport`, or `NoListener` (only for `Unavailable`).
- The answer is judged by HTTP status and GraphQL error code, never by message text. A readable code is the text `extensions.code`, with the optional text `extensions.reason` and the optional object `extensions.params`. A `reason` or a `params` that is null is absent. An error whose `reason` is not text or whose `params` is not an object has no readable code, and every other key of `extensions` is dropped. An error whose readable code is the text `UNAUTHENTICATED`, wherever it stands in the list, or an HTTP 401 whatever its body, is `Unauthenticated`, with any status. `Unauthenticated` wins over `Refused`. The first error with a readable code is `Refused`, and the `data` beside it is dropped. `Refusal` holds `code`, `reason`, `params` (empty when absent), `message` and the origin. An HTTP 403 with no readable code is `Refused` with the code `FORBIDDEN`, no reason, no params and `CodeOrigin::Connection`. A 2xx answer whose body was cut is `Lost`. A 2xx answer that cannot be read, that holds no `data` (absent or null) and no error, whose `data` is not an object, or whose errors carry no readable code, is `Unreadable`. An empty or null `errors` is no error. Another non-2xx status with no readable code is `Lost` with the transport as cause.
- The only headers are `Host`, `Content-Type`, `Content-Length` and `Authorization`. No Passport header is set. The bearer header is marked sensitive and is not shown by the `Debug` of the request.
- A bus request is `InvalidInput`, and nothing is sent, when the wait is zero, when the subject is empty, holds a whitespace (any Unicode whitespace), holds an empty token, or holds a token that is `*` or `>`, or when the body is larger than the maximum payload of the bus. It is `Unavailable` when the bus connection is not connected (cause `Transport`) or no service listens (cause `NoListener`). It is `Lost` when no reply comes within the wait (cause `WaitFired`) or when the bus session was cut at any moment during the request (cause `Transport`): a cut during a request is known only when its wait ends. The connection never repeats a request after a reconnection and runs no reconnect loop. It creates no stream, no bucket and no consumer. A wait too long to be added to the clock never fires.
- No text written by the library holds a token that a credential gave: the texts of `InvalidInput`, `Unavailable`, `Lost`, `Unreadable`, of the `Credential` errors that the library writes, and the `Debug` of a `Connection`, a `Token` and a `FixedToken`. The text of a failing credential and the text of the service (code, reason, parameters, message) are carried as they are given, and a token in them is not found or removed. `Debug` of a connection prints nothing of the credential or of the bus connection. The library writes no log.
- The dependency gate: `br-runner-connection` depends on no library of the `br-llm-clients` family, directly or transitively, not even for its tests. `.github/scripts/check-no-llm-clients.sh` reads `cargo metadata` and fails the build if one is reachable.
- Nothing is sent again: not after a lost answer, a rejection or a reconnection. After the gateway rejects a token, no other token is asked for that call.

## Limits of this version

| What | State |
|---|---|
| `FixedToken::new` | Takes any text. An empty token, or one that is not a header value, is refused at the first call as `Credential` (no usable credential), not at building. The sealed intent says a fixed token refused at building; a correction is proposed (pending) to remove that sentence, because every credential already fails closed at the call. |
| Service account | Typed `ServiceAccountId` of `br-core-kernel`, carried as given. The sealed intent says "a text" while the crate carries the typed `ServiceAccountId`; a correction is proposed (pending) to make the service account that typed value. |
| Bus `Unauthenticated` | Not built: the intent keeps it To Be Implemented; a request refused by the bus ends as `Lost` by its wait, or `Unavailable` when the bus is not connected. `async-nats` tells a refusal of the credentials of the bus, or of the right to send on a subject, only to the event callback that the importer sets when it builds the client, and a `Client` has no accessor for it. |

## Why

| Thing | Why it is the way it is |
|---|---|
| direct `async-nats` in `bus` | the 2026-08-14 runner-transport exception covers the `svc-jobs` runner channels; its extension to this crate is pending ratification: wire outside `integration.*`, faces the runners, shape from the contract crate |
| one TCP connection per call, no pool | The connection runs no task and keeps nothing after a call, so an abandoned call leaves nothing behind |
| no retry | A resend is the importer's decision; only the importer knows if the call was applied (`Lost`) |
| bus `_ =>` arm of `bus_request` | An `async-nats` request error of kind `Other` (for example a closed client) ends as `Lost` with cause `Transport`, even when nothing was sent. The library cannot tell whether the request left, so it takes the conservative direction: the importer must treat it as possibly applied |
| text of the service not scrubbed | The library cannot find a token in a text it did not write; carrying the text as given keeps `Refusal` faithful to what the service said |
