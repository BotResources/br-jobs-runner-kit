# br-runner-connection

The one connection of a runner to the platform. Runner code builds one `Connection` and gives it to `br-runner-threads` and `br-runner-jobs`. It does the bearer-authenticated GraphQL request, maps every failure to a small error vocabulary, keeps every token out of every text, and gives the bus connection to the runner libraries. It knows no thread, job, run, mail, agent or Passport, and it sends each request once.

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

- The endpoint is an absolute `http` or `https` URL, else `InvalidInput` at building. The library reads no environment variable and no file.
- One new HTTP/1.1 connection per request, no pool and no task. The reply wait counts from the moment the token is in hand and covers the connection, the request and the whole answer.
- Before sending (connect failure, failed handshake, wait fired before the connection was ready) is `Unavailable`. After the connection was ready, an interrupted request, a protocol error or the wait firing is `Lost`. Both say whether the wait fired or the transport failed.
- The answer is judged by HTTP status and GraphQL error code, never by message text. `UNAUTHENTICATED` or HTTP 401 is `Unauthenticated` and wins over `Refused`. The first error with a code is `Refused`; an HTTP 403 with no readable code is `Refused` with the code `FORBIDDEN` and `CodeOrigin::Connection`. A 2xx answer that cannot be read, or whose errors carry no code, is `Unreadable`. Another non-2xx status with no readable code is `Lost`.
- `Refusal::params` holds every key of the error extensions except `code`.
- The only headers are `Host`, `Content-Type`, `Content-Length` and `Authorization`. No Passport header is set.
- The bus request is `Unavailable` when the bus connection is not connected or no service listens, `Lost` when no reply comes within the wait or the bus session was cut. The connection never repeats a request after a reconnection.
- A token the credential gave is replaced by `[redacted]` in every text, code and parameter of an error of the request it served. `Debug` of a connection, a token or a fixed credential holds no token. The library writes no log. The text a failing credential gives is carried as given.
- Nothing is sent again: not after a lost answer, a rejection or a reconnection.
