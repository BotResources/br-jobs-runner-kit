# Changelog

## 0.1.1

Brings the code in line with the corrected intent of 0.1. The public API of 0.1.0 is unchanged; `Cause` gets one more value (it is `#[non_exhaustive]`).

### Changed

- `UNAUTHENTICATED` is read from a readable code only. An error whose `reason` is not text, or whose `params` is not an object, has no readable code: it is no longer `Unauthenticated`, and it does not win over an error that has a readable code (`Unreadable` on a 2xx, `Lost` on another non-2xx status).
- A `data` that is not an object, with no error, is `Unreadable` (it was given as data).
- The text of the service (message, code, reason, parameters) is carried as the service gives it. The token is no longer replaced by `[redacted]` in it, and the scrub of error texts is removed. No text that the library writes holds a token, and tests search every one of them.
- `graphql` checks its input before it asks the credential: an empty document, or variables that are neither a JSON object nor `null`, are `InvalidInput` and nothing is sent. `null` variables are sent as `{}`.
- `bus_request` checks the subject before the bus sees it: empty, any Unicode whitespace, an empty token, or a token that is `*` or `>` is `InvalidInput`. A body larger than the maximum payload of the bus is `InvalidInput` and nothing is sent.
- A request that no service listens to is `Unavailable` with the new cause `Cause::NoListener` (it was `Transport`).

### Added

- `Cause::NoListener`.
- The four examples of the intent in `examples/`: `build_connection` (the quickstart of the README, checked word for word by a test), `graphql_call`, `custom_credential`, `one_connection_for_all`. The tests run them against a real local HTTP endpoint and a real NATS server.
- Tests: no redirect is followed, input checks, subject and size checks, the three causes, the text of the service carried as given.

### Bus access

- The bus connection stays the `async_nats::Client`. `async_nats` is named in one module only, `src/bus/`, and the crate has no subject, stream or bucket name of its own. The request and the accessor stay generic; only `br-runner-jobs` (`runner_transport`) and `br-runner-threads` (`chunk_lane`) call them. This is the runner-transport exception (jx 2026-08-14, extended 2026-10-31); the README has a "Bus access" paragraph and a Why row.
- The dependency `br-util-nats-fabric` is removed: the connection no longer builds a Fabric.

### Not done in this version

- `FixedToken::new` takes any text; the refusal of an empty or unsendable token happens at the first call, as `Credential`. Refusing it at building needs a fallible `new`, a breaking change.
- The service account is the typed `ServiceAccountId`. The intent calls it a text; changing the type is a breaking change.
- A refusal of the bus (credentials, or the right to send on a subject) is not reported as `Unauthenticated`: `async-nats` gives it only to the event callback that the importer sets when it builds the client.

## 0.1.0

- First release: the one connection of a runner to the platform. A bearer-authenticated GraphQL call to the gateway, one request with one reply on core NATS, the credential seam and a fixed-token credential, one error type that tells apart what was sent from what was not.
