# br-runner-toolkit

The main loop of a runner with the jobs service. The importer gives a ready `JobsClient` (from `br-runner-jobs`), a graph, a function that starts a run, and `Settings`. The toolkit keeps the runner present in the jobs service, takes Runs in parallel slots, drives each Run through the graph executor, sends the facts of each Run to jobs, and drains on shutdown. It talks to the jobs service only. It names no thread, no mail transport, no stream and no subject, and it does not name `async-nats`.

## Install

```toml
br-runner-toolkit = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-toolkit", tag = "br-runner-toolkit/v0.1.0", version = "0.1.0" }
br-runner-jobs = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-jobs", tag = "br-runner-jobs/v0.1.0", version = "0.1.0" }
br-runner-connection = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-connection", tag = "br-runner-connection/v0.1.1", version = "0.1.1" }
br-graph-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-graph-contract", tag = "br-graph-contract/v0.1.0", version = "0.1.0" }
br-agentic-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-contract", tag = "br-agentic-contract/v0.2.0", version = "0.2.0" }
br-core-kernel = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-kernel", tag = "br-core-kernel/v0.1.0", version = "0.1.0" }
async-nats = "0.48"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal"] }
tokio-util = "0.7"
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pins above are the ones this crate is built with. The `test-host` feature adds `TestHost` and `advance_clock` and turns on the test clock of tokio; use it from `dev-dependencies` only.

## Quickstart

This is `examples/serve_runs.rs`. It builds the `Settings`, gives the `StartArgs` a graph, a run start function and a shutdown token, and runs `RunnerBuilder::start` until the token is cancelled. The graph and the start function are in `examples/shared/greeter.rs`. The same code serves a jobs service in push mode and in claim mode: the mode comes from jobs.

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};
use br_runner_jobs::JobsClient;
use br_runner_toolkit::{DrainReport, RunnerBuilder, Settings, StartArgs};
use tokio_util::sync::CancellationToken;

#[path = "shared/greeter.rs"]
#[allow(dead_code)]
mod greeter;

pub fn settings() -> Result<Settings, br_runner_toolkit::Error> {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(4)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(5))
        .claim_wait(Duration::from_secs(10))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
        .build()
}

pub async fn serve(
    client: JobsClient,
    shutdown: CancellationToken,
) -> Result<DrainReport, Box<dyn std::error::Error + Send + Sync>> {
    let args =
        StartArgs::new(greeter::graph(None)?, greeter::starter(), false).with_shutdown(shutdown);
    Ok(RunnerBuilder::new(client, settings()?).start(args).await?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    let [
        _,
        endpoint,
        nats_url,
        token,
        account,
        runner_type,
        instance_key,
    ] = args.as_slice()
    else {
        return Err(
            "usage: serve_runs <endpoint> <nats-url> <token> <account-uuid> <runner-type> <instance-key>"
                .into(),
        );
    };
    let connection = Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token.as_str())),
        async_nats::connect(nats_url).await?,
        ServiceAccountId(account.parse()?),
        Duration::from_secs(30),
    )?;
    let client = JobsClient::new(
        runner_type,
        instance_key,
        Duration::from_secs(10),
        connection,
    )
    .await?;
    let shutdown = CancellationToken::new();
    let stopper = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stopper.cancel();
    });
    let report = serve(client, shutdown).await?;
    println!("{} runs left in flight", report.runs.len());
    Ok(())
}
```

Two more examples: `restartable_agent` (an instance lookup, a lifecycle hook, a restartable runner and a custom answer) and `test_host` (one run under `TestHost`). The tests run all three, and the first in push mode and in claim mode.

## What the runner does

### Start

`RunnerBuilder::start` reads the time to live of the presence from jobs, then starts the presence task, the feed of the dispatch mode, the feed of the cancel orders and one slot task for each slot of `max_concurrent_runs`. It returns only after the shutdown and the drain. If the time to live cannot be read, it returns `JobsFailed`. `Settings` is built with `Settings::builder()`: a missing value is `SettingMissing`; zero slots or a zero `ack_wait`, `claim_wait`, `outcome_wait` or `retry_wait` is `SettingInvalid`. `drain_timeout` and `cancel_grace` may be zero.

### Presence

The runner publishes its presence (the `runner_version` of the settings, the status `Ready` or `Draining`, and the capacity `max_concurrent_runs`) every third of the time to live. A change of status is published at once. A publication still in flight at the next beat is dropped. A failed publication is logged and the next beat tries again.

### When the runner asks for work

A slot asks for work only when the dispatch mode is known and is not "no entry", and the replay of the cancel orders is complete. Until then no trigger is read and no claim is sent. If the dispatch mode becomes "no entry" or the cancel feed restarts, a slot stops asking until the condition returns.

- Push mode: the slot reads one trigger at a time from the trigger feed. A feed that cannot be opened or that ends is opened again after the next beat. A trigger that cannot be decoded is logged and skipped. A trigger for a Run that the runner already holds is acknowledged only when the started fact of that Run has ended; it never starts a second Run.
- Claim mode: the slot sends a claim with a new claim id. An empty answer is followed by a claim with a new id. A "busy" refusal repeats the same claim id after `retry_after_ms`, or after the next beat if jobs gives none. An "instance unknown" refusal and a missing answer repeat the same claim id after the next beat. A "claim spent" refusal is followed by a new claim id. Any other refusal stops the claims until the dispatch mode feed gives a new value.

### A Run

For each Run the runner, in this order: registers the Run (a cancel order already known cancels it before the graph starts), makes the delivery queue (its first write is the started fact), acknowledges the trigger (push mode only, after the started fact), reads the saved state (a restart only), starts the graph, waits until all the instances have returned and the lifecycle hooks have returned, waits until the queue holds nothing, hands over the answer, the saved state and the completed or failed fact, and forgets the Run once the queue is drained.

- If the started fact is refused by jobs, the Run is not executed; in push mode the trigger is acknowledged and dropped. If it fails for another reason, the Run is not executed and the trigger is not acknowledged.
- A Run that was cancelled ends with no completed and no failed fact. `HostCancel` and a cancel order of jobs both cancel; `cancel_grace` is the time the executor gives the nodes to return.
- The failed fact is permanent unless stated, with these reason codes: `node_failed`, `node_refused_an_input`, `node_could_not_start`, `node_vanished`, `update_refused`, `edge_failed`, `checkpoint_not_built`, `sink_refused`, `run_failed` (no failure was given), `start_refused`, `panic`, `saved_state_invalid`, `saved_state_not_made`, `answer_not_made`, `answer_refused` and `saved_state_refused`. `saved_state_unavailable` is transient. The diagnostic is the text of the failure. A `FailureMapping` may change the kind, the reason code, the parameters, the diagnostic and the retry delay; a panic in the mapping gives the reason code `panic`.
- A sync call (mode `SYNC`) gets an answer: by default the state fields as JSON text with no attachments, or the content of the function given to `with_answer`. An error of that function gives `answer_not_made`; a panic gives `panic`.
- A runner that can restart, when at least one agent of the Run has an inbox, saves a document with the state fields and the agents of the Run at the end. A restart reads that document; a restart whose document cannot be read or decoded fails with `saved_state_invalid`, one whose read could not be made fails with `saved_state_unavailable`, and a restart starts at the entry nodes of the graph with the restored fields given to the run start function. `with_state_limit` bounds the state size.
- If jobs refuses the answer or the saved state, the completed fact is removed and a failed fact (`answer_refused` or `saved_state_refused`) is sent in its place.
- The run start function and a lifecycle hook may panic; the panic is caught and logged.

### Lookup and hooks

`RunnerBuilder::instance_lookup` gives an `InstanceLookup` a node uses, while it creates its instance, to get the `AgentBook` and the `RunContext` of its Run from an `InstanceId`, and the `BookLookup` that `br-runner-jobs` needs to make an agent entry. Before `start` the lookup returns `NotStarted`. For an instance the runner does not hold it waits, and that includes the instances of a Run the runner has dropped. Between the return of the executor (or the abort of a drain) and the drop of the Run it returns `InstanceEnded`. A `RunContext` holds weak references to the delivery queue and the agent book: author code cannot keep a Run alive with it, and `agent_book`, `caller_handle`, `plan_declared` and `step_started` return an error after the Run ended. Its other calls (the ids, `passport`, `payload`, `log`, `mail_handle`) go on working.

A lifecycle hook runs once for each instance when the executor has returned for it, with the node name and the agents the instance registered. The Run is not settled until every hook has returned.

### Drain

On shutdown (the token, or `with_process_signals`: SIGTERM or SIGINT on unix, ctrl-c elsewhere) the runner cancels the drain token, sets the presence to `Draining`, stops taking Runs (a trigger already read is given back), and waits up to `drain_timeout` for the Runs and for their closings. At the timeout it aborts what remains, builds a `DrainReport` (for each run: ids, whether it was aborted, the instances that did not return, the writes not sent), deletes the presence and returns the report.

### Resilience

Every port is async. Nothing waits for jobs except the time to live at start and the work the Run cannot do without (its started fact). A write the queue cannot send for a reason that can pass is sent again by the queue; the toolkit does not send a write twice itself.

## Test host

With the feature `test-host`, `TestHost::new(client, settings_builder, start_args)` runs the same loop with one slot and no drain time, takes one Run, and returns `HostRun { events, drain }`. `events` is one ordered list: the reports of the queue, the acknowledgement or give-back of the trigger, the claims, the presence publications and deletions, and the run logs. `cancel_handle()` cancels the Run. `advance_clock(duration)` moves the paused clock of tokio.

## Why

| Thing | Why it is the way it is |
|---|---|
| No `async-nats` and no subject in this crate | Bus access is in `br-runner-jobs`; the toolkit sees only its typed client. |
| `RunContext` holds weak references | A node keeping the queue or the book would stop the queue from draining at the end of the Run. |
| A busy refusal without delay waits for the next beat | A fixed delay would be an invented limit; the beat is the one rhythm the runner already has. |
| The default answer is the fields as JSON text | The graph has no answer node; the state is the only result every graph has. |
| The failed fact for a refused answer is sent after the completed fact is removed | The queue removes the completed fact when a write it depends on is refused; the failed fact is then the only way jobs learns why the Run did not complete. |
| The test host takes one Run and drains with no time limit | A test needs a stop point it does not have to time, and a drain that could abort would hide the report. |
