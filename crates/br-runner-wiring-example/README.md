# br-runner-wiring-example

An end-to-end wiring example for a runner author. Not published (`publish = false`), not a library.

It wires the toolkit (`TestHost` over a graph with one programmatic node), the jobs adapters, the Threads adapter and the harness, over the in-process doubles the crates ship. Two variants run the same flow:

| Variant | Wiring |
|---|---|
| `Harness` | `NodeEntry::with_mail` gives the mail port; the harness takes the mail port and the Threads adapter as its history (`HarnessHistory`); the harness incoming port marks a mail read only when `stored` of the history answers `Stored` |
| `Gate` | `NodeEntry::with_inbox` with a `MarkGate` made from `ThreadAccessAdapter::entry_stored` (`Stored` or `Dropped`); no harness |

The node lists the unread mail, writes it as an input turn to the thread, marks it read, and waits until the listing no longer holds it.

`cargo run -p br-runner-wiring-example --example end_to_end` runs both variants. `cargo test -p br-runner-wiring-example` proves: a mail is marked read in jobs only after the thread entry is stored, a mail stays unread while the thread write is not taken, a mail stays unread when the store fails, and the toolkit drives the run.

## Pins

| Crate | Tag |
|---|---|
| `br-agentic-contract` | `br-agentic-contract/v0.3.0` |
| `br-agentic-harness-contract` | `br-agentic-harness-contract/v0.2.0` |
| `br-agentic-harness` | `br-agentic-harness/v0.2.0` |
| `br-runner-jobs-client` | `br-runner-jobs-client/v0.1.1` |
| `br-runner-jobs` | `br-runner-jobs/v0.5.0` |
| `br-runner-threads` | `br-runner-threads/v0.8.0` |
| `br-runner-toolkit` | `br-runner-toolkit/v0.3.1` |
