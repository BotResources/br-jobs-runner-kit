# br-runner-threads

The typed client of the Threads service for code that runs inside a runner. It creates threads, writes whole messages or streams, closes a message with a named reason, reads the projected context, sets or fails the title, and requests an artifact upload. Conversation types are those of `br-llm-messages`; thread references are those of `br-agentic-contract`. No wire type appears in the API.

It is thin. It sends what the importer gives, sends nothing twice, and invents no id, limit or timeout. The sealed intent of the minor is the contract; this file maps it to the code.

## Surface

| Item | Role |
|---|---|
| `ThreadsClient::new(actor, bus, graphql)` | Builds the client. Publishes nothing, runs no timer, is `Clone`. |
| `create_thread`, `write_message` | Bus commands. Return `Confirmed { command_id, status }`. |
| `open_stream` | Bus command with status `STREAMING`. Returns a `StreamHandle` only after the confirmation. |
| `close_message(&Closing, Option<&StreamHandle>)` | Closes with `Completed`, `BudgetExhausted`, `Failed`, `Interrupted` or `Cancelled`. |
| `read_context`, `set_title`, `fail_title_job`, `request_upload` | GraphQL operations. |
| `StreamHandle` | `delta`, `entry`, `annotation`, `keepalive`, `writer()`, `sink_factory()`. |
| `StreamWriter` | Cloned write side: `entry`, `annotation`, `keepalive`. Shares the sequence and the seal. |
| `SinkFactory` / `ResponseSink` | `sink()` gives a sink; `send(ResponseDelta)` publishes one delta chunk and waits for the broker. |
| `transport::{BusTransport, GraphqlTransport}` | The two seams the importer can replace. |
| `transport::NatsBus`, `transport::HttpGraphql` | Ready transports. |
| `transport::{MemoryBus, MemoryGraphql}` | In-memory transports for tests. They record and answer as the test says and apply no Threads rule. |
| `Error` | The single error type. |

```ignore
let client = ThreadsClient::new(actor, Arc::new(NatsBus::connect(nats).await?), Arc::new(graphql));
let handle = client.open_stream(&open).await?;
let sink = handle.sink_factory().sink();
handle.keepalive().await?;
client.close_message(&closing, Some(&handle)).await?;
```

## Rules the code keeps

- Every id of a new object must be a UUIDv7 and is refused before sending otherwise (`InvalidInput`). Reference ids are sent as given.
- A command ends with its answer. Without `wait`, the operation waits for the answer or for the closing of the answer subscription. With `wait`, a missing answer is `AnswerLost`.
- Chunks are numbered from 0 without a gap, one at a time per handle and its writers, each ending at the broker acknowledgement. A chunk the broker does not acknowledge breaks the handle (`StreamBroken`), and so does a chunk operation abandoned after it took its number.
- A closing is sent after every chunk that took a number and after each acknowledgement. While a closing waits for its answer, the handle takes no chunk (`StreamClosed`). The handle is free again only when every closing sent with it was answered `Refused`. A closing answered `AnswerLost` or `Unreadable`, or abandoned after sending, closes the handle for good. A confirmed closing spends it. A closing refused before sending changes nothing.
- `Completed` and `BudgetExhausted` need the handle and go on the completion command with the seal. A handle with no chunk has no seal, so the reason goes on the fail command. `Failed`, `Interrupted` and `Cancelled` go on the fail command, carry no seal, and need no handle.
- The closing detail is checked by `br-llm-messages`: `Failed` requires one; no detail may be blank.
- A missing thread in `read_context` is `Ok(None)`. A document `br-llm-messages` refuses is `Unreadable` with its stable reason.
- GraphQL: one token per request from the token cache, sent as bearer. After a rejection no other token is fetched and nothing is sent again. The secrets that the token source declares are removed from error texts; a token it does not declare is not.

## Errors

| Error | Meaning |
|---|---|
| `InvalidInput` | Refused before sending by a rule above, or by `br-llm-messages` (stable reason as text). |
| `Refused(Refusal)` | Threads answered with a code, parameters, and the command id for a command. A 403 with no readable code is `FORBIDDEN`. With several GraphQL errors, the first one that has a code. |
| `Unavailable` | Nothing was sent and Threads received nothing. |
| `AnswerLost` | Sent, no answer. It may or may not have been applied. |
| `StreamBroken` | A chunk was not acknowledged. The handle stays broken. |
| `StreamClosed` | The handle is closed, for now or for good. |
| `Auth` | The token cache gave no token. |
| `Unauthenticated` | HTTP 401, or a GraphQL error coded `UNAUTHENTICATED`. Wins over `Refused`. |
| `Unreadable` | An answer or document that cannot be read. It may have been applied. |
| `Infra` | A stream the bus transport binds is missing or cannot be bound. |

## Why

| Thing | Why it is the way it is |
|---|---|
| `NatsBus` binds streams and creates none | Streams are declared by gitops. A missing stream is `Infra`, and an absent subject stream is `Unavailable`. |
| One shared answer subscription | Answers are matched by command id. Each command registers its waiter before it publishes, and a dropped waiter is forgotten. |
| Chunks are published on the streaming stream directly, with `Nats-Msg-Id` `{message_id}:{seq}` | The fabric has no writer-side lane publish. The id lets the broker drop a duplicate of one chunk. |
| `ResponseSink` is this crate's own type | `br-agentic-contract` has an infallible sink; a chunk write can fail. |
| A closing waits on a gate | The seal covers exactly the chunks that took a number before it. |
| `AnswerLost` closings close the handle for good | Threads may have applied the closing. |
| Declared token secrets are removed from GraphQL error texts | The token source can echo a token in its own failure text. Only what the source declares is known to the client. |
| The member `display_name` is optional and `in_roster` is read | Threads may send a member that is not in the roster. |
