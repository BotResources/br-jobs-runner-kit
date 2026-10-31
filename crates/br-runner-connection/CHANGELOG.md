# Changelog

## 0.1.1

Brings the code in line with the corrected intent of 0.1. The public API of 0.1.0 is kept; the only addition is `Cause::NoListener` (additive, the enum is `#[non_exhaustive]`).

### Changed

- `UNAUTHENTICATED` is read from a readable code only. An error whose `reason` is not text, or whose `params` is not an object, has no readable code: it is no longer `Unauthenticated`, and it does not win over an error that has a readable code (`Unreadable` on a 2xx, `Lost` on another non-2xx status).
- A `data` that is not an object, with no error, is `Unreadable` (it was given as data).
- The text of the service (message, code, reason, parameters) is carried as the service gives it. The token is no longer replaced by `[redacted]` in it and the scrub of error texts is removed, so an importer must not log a gateway error text that could echo a secret it sent. No text that the library writes holds a token, and tests search every one of them.
- `graphql` checks its input before it asks the credential: an empty document, or variables that are neither a JSON object nor `null`, are `InvalidInput` and nothing is sent. `null` variables are sent as `{}`.
- `bus_request` checks the subject before the bus sees it: empty, any Unicode whitespace, an empty token, or a token that is `*` or `>` is `InvalidInput`. A body larger than the maximum payload of the bus is `InvalidInput` and nothing is sent.
- A request that no service listens to ends with the new `Cause::NoListener` instead of `Cause::Transport`; it is still `Unavailable`. A malformed `UNAUTHENTICATED` on a non-2xx status is now `Lost` (it was `Unauthenticated`).

### Added

- `Cause::NoListener`.
- A dependency gate: `.github/scripts/check-no-llm-clients.sh` fails the build when a library of the `br-llm-clients` family is reachable from `br-runner-connection`, directly or transitively, dev-dependencies included. Its test (`test-no-llm-clients.sh`) covers a direct and a transitive dependency and the real crate.
- The four examples of the intent in `examples/`: `build_connection` (the quickstart of the README, checked word for word by a test), `graphql_call`, `custom_credential`, `one_connection_for_all`. The tests run them against a real local HTTP endpoint and a real NATS server.
- Tests: no redirect is followed, input checks, subject and size checks, the three causes, the text of the service carried as given.

### Bus access

- The bus connection stays the `async_nats::Client`. `async_nats` is named in one module only, `src/bus/`, and the crate has no subject, stream or bucket name of its own. Subject names must come from the contract crate of the service that owns the wire, and the published language and `integration.*` must go through `br-util-nats-fabric`, never through this request. The request and the accessor stay generic. They are meant to be called only by the `runner_transport` module of `br-runner-jobs` and the `chunk_lane` module of `br-runner-threads`, when they take a `Connection`; neither library calls this crate yet.
- Basis: the 2026-08-14 runner-transport exception covers the `svc-jobs` runner channels; its extension to this crate is pending ratification. The README has a "Bus access" paragraph and a Why row.
- The dependency `br-util-nats-fabric` is removed: the connection no longer builds a Fabric.

### Not done in this version

- `FixedToken::new` takes any text; the refusal of an empty or unsendable token happens at the first call, as `Credential`. The sealed intent says a fixed token refused at building; a correction is proposed (pending) to remove that sentence, because every credential already fails closed at the call.
- The sealed intent says the service account is "a text" while the crate carries the typed `ServiceAccountId` of `br-core-kernel`; a correction is proposed (pending) to make the service account that typed value.
- Bus `Unauthenticated`: not built: the intent keeps it To Be Implemented; a request refused by the bus ends as `Lost` by its wait, or `Unavailable` when the bus is not connected.

## 0.1.0

- First release: the one connection of a runner to the platform. A bearer-authenticated GraphQL call to the gateway, one request with one reply on core NATS, the credential seam and a fixed-token credential, one error type that tells apart what was sent from what was not.
