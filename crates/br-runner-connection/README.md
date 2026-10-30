# br-runner-connection

The one connection of a runner to the platform. Runner code builds one `Connection` and gives it to `br-runner-threads` and `br-runner-jobs`. It does the bearer-authenticated GraphQL request, maps every failure to a small error vocabulary, scrubs every text of an error of the request it served by exact match of the token, and gives the bus connection to the runner libraries. It knows no thread, job, run, mail, agent or Passport, and it sends each request once.

## Install

```toml
br-runner-connection = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-connection", tag = "br-runner-connection/v0.1.0", version = "0.1.0" }
```

## Surface

| Item | Role |
|---|---|
| `Connection::new(endpoint, credential, bus, service_account, reply_wait)` | Builds the connection. Every value is required. Sends nothing, binds nothing, runs no task. `Clone`. |
| `Connection::graphql(document, variables)` | Posts the document once, with the bearer token, and gives the `data` as JSON. |
| `Connection::bus_request(subject, body, wait)` | One request on core NATS, one reply. Gives the body of the reply. |
| `Connection::bus`, `Connection::service_account` | Give back what the connection was built with. |
| `Credential` | The seam: gives one bearer token, asked before each GraphQL request and only then. |
| `FixedToken` | A credential that holds one fixed secret token. |
| `Error` | The single error type. |

```ignore
let connection = Connection::new(
    "https://gateway.example.test/graphql",
    Arc::new(FixedToken::new(token)),
    nats_client,
    service_account,
    Duration::from_secs(30),
)?;
let data = connection.graphql("query { me { id } }", json!({})).await?;
```

## Rules the code keeps

- The endpoint is an absolute `http` or `https` URL with no user information and no fragment, whose port, when it has one, is a number from 1 to 65535 (a missing port is 80 for `http` and 443 for `https`, never the fallback of a bad one), and whose bracketed host, when it has one, is an IPv6 address with nothing but a port after the bracket, else `InvalidInput` at building; a query without a path is sent as `/?query`; the refusal text does not repeat the URL. The library reads no environment variable and no file. A zero reply wait is `InvalidInput` at building.
- With an `http://` endpoint the bearer token travels in clear to the host the importer gave, whatever that host is. The library does not refuse `http` for any host.
- With `https://` the server certificate is checked against the webpki roots only (no system store, no private authority, no way to add one) and against the host of the endpoint; a failure is `Unavailable` with the transport as cause.
- One new TCP connection and, for `https`, one TLS handshake per call; HTTP/1.1, no pool, no task. The reply wait counts from the moment the token is in hand and covers the connection, the request and the whole answer. A wait too long to be added to the clock (`Duration::MAX`) never fires. The library sets no bound on the size of a response; the wait bounds the time to read it.
- Before sending (connect failure, failed handshake, wait fired before the connection was ready) is `Unavailable`. After the connection was ready, an interrupted request, a protocol error or the wait firing is `Lost`. Both say whether the wait fired or the transport failed.
- The answer is judged by HTTP status and GraphQL error code, never by message text. A GraphQL error whose `extensions` code is the text `UNAUTHENTICATED`, whatever its `reason` and `params`, or an HTTP 401 whatever its body, is `Unauthenticated`, with any status. `Unauthenticated` wins over `Refused`. The first error with a readable code is `Refused`. A readable code is the text `extensions.code`, with the optional text `extensions.reason` and the optional object `extensions.params`. A `reason` or a `params` that is null is absent. an error whose `reason` is not text or whose `params` is not an object has no readable code, and every other key of `extensions` is dropped. `Refusal` holds `code`, `reason`, `params` (empty when absent), `message` and the origin. An HTTP 403 with no readable code is `Refused` with the code `FORBIDDEN`, no reason, no params and `CodeOrigin::Connection`. A 2xx answer that cannot be read, that holds no `data` (absent or null) and no error, or whose errors carry no readable code, is `Unreadable`. Another non-2xx status with no readable code is `Lost`.
- The only headers are `Host`, `Content-Type`, `Content-Length` and `Authorization`. No Passport header is set. The bearer header is marked sensitive and is not shown by the `Debug` of the request.
- The bus request is `InvalidInput`, and nothing is sent, when the wait is zero or the bus refuses the subject. It is `Unavailable` when the bus connection is not connected or no service listens, `Lost` when no reply comes within the wait or the bus session was cut. A cut during a request is known only when its wait ends; the request is then `Lost` with the transport as cause. The connection never repeats a request after a reconnection. It creates no stream, no bucket and no consumer. A wait too long to be added to the clock never fires.
- A token the credential gave is replaced by `[redacted]` in every text, code, reason and parameter of an error of the request it served, by exact match of the token: a transformed form of the token (encoded, split, truncated) is not found. `Debug` of a connection, a token or a fixed credential holds no token. The library writes no log. The text a failing credential gives is carried as given.
- Nothing is sent again: not after a lost answer, a rejection or a reconnection.
- The public error types are `#[non_exhaustive]`: a variant or a field can be added in a minor.
