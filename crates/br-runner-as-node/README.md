# br-runner-as-node

**Known gap.** In a map-reduce, an instance of `RunnerAsNode` waits on its caller source until the host triggers the cancel view. The host makes the caller source from the run context that its instance lookup gives for an instance id, and the ids of the instances that a map-reduce creates itself are not yet known to that lookup. Outside a map-reduce the node runs as described below.

`RunnerAsNode` is a node of `br-graph-contract` that makes another runner do one step of a graph. Use it when runner C must do one step of the graph of runner A. Make the node once, with a name, the runner type of C, a request function, a response function and a caller source. For an agent of a ReAct node, use `br-runner-as-tool`.

The node is of kind `programmatic` and of subkind `runner_call`. It is generic over the fields of its graph, so one type serves as an outer node and as the internal node of a map-reduce. It is not a port: it implements no trait of an agentic contract.

## Install

```toml
br-runner-as-node = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-as-node", tag = "br-runner-as-node/v0.1.0", version = "0.1.0" }
br-runner-jobs = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-jobs", tag = "br-runner-jobs/v0.1.0", version = "0.1.0" }
br-graph-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-graph-contract", tag = "br-graph-contract/v0.1.0", version = "0.1.0" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pins above are the ones this crate is built with. The public API names `CallerHandle`, `Content`, `ResultRef` and `FailureReportView` of `br-runner-jobs`, and the types of `br-graph-contract`. `SubjectSegment`, the validated runner type, is re-exported.

## Quickstart

This is `examples/call_a_runner_as_node.rs`. It makes a node that sends the question of the state to the runner type `analyst` as a payload kept by jobs, waits for the end of the child Job, and puts the inline answer in the state. A failed, cancelled or refused call fails the instance with the message of jobs. The example takes the caller source as a parameter; to run it needs a caller source made by the host. The tests run it against an in-memory caller handle.

```rust,no_run
use br_graph_contract::{Fields, Reason};
use br_runner_as_node::{CallerSource, JobSpec, Outcome, Payload, RunnerAsNode, SubjectSegment};
use br_runner_jobs::Content;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub question: String,
    pub answer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReviewUpdate {
    pub answer: Option<String>,
}

impl Fields for Review {
    type Update = ReviewUpdate;

    fn empty_update() -> ReviewUpdate {
        ReviewUpdate::default()
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: ReviewUpdate) -> Result<(), Reason> {
        if update.answer.is_some() {
            self.answer = update.answer;
        }
        Ok(())
    }
}

pub fn call_a_runner_as_node(
    caller_source: impl CallerSource,
) -> Result<RunnerAsNode<Review>, Box<dyn std::error::Error>> {
    let runner_type = SubjectSegment::runner_type("analyst")?;
    Ok(RunnerAsNode::<Review>::new(
        "ask_the_analyst",
        runner_type,
        |state| {
            let question = state.fields().question.clone();
            async move {
                let payload = Content {
                    message: question,
                    attachments: Vec::new(),
                };
                Ok(JobSpec::new(Payload::Kept(payload)))
            }
        },
        |_state, outcome| async move {
            match outcome {
                Outcome::Completed {
                    inline: Some(content),
                    ..
                } => Ok(ReviewUpdate {
                    answer: Some(content.message),
                }),
                Outcome::Failed { message, .. } | Outcome::Cancelled { message } => {
                    Err(message.into())
                }
                Outcome::Refusal(refusal) => Err(refusal.message.into()),
                _ => Err("the analyst gave no inline answer".into()),
            }
        },
        caller_source,
    ))
}

fn main() {
    println!("call_a_runner_as_node(caller_source) needs a caller source made by the host");
}
```

## What an instance does

An instance does one step and sends one event. In this order:

1. It waits for the first input, closes its input, and keeps the last state it was given.
2. It awaits your request function with that state. The function gives a `JobSpec`: the payload and the configuration.
3. It asks the caller source for the caller handle of the instance.
4. It opens the end wait for a new child Job id.
5. It hands over the upload, when the payload is `Payload::Kept`, and then the create of a sync child Job of your runner type.
6. It waits for the end of the child Job.
7. When the end is completed, it hands over the finish of the child Job.
8. It awaits your response function with the same state and the `Outcome`.
9. It sends the event: `Result` with the update, or `Failed` with the error.

The node never sends the event `Refused`.

### Waits and hand-overs

The node blocks only where it cannot go on without the answer: the caller source, the end, and your two functions. The upload, the create, the finish and the cancel are handed over with no wait; the delivery queue of `br-runner-jobs` sends them in hand-over order and sends a call again when it must. The node adds no timeout and no retry count and sends nothing again. The cancel view ends every wait of the node, the waits for your two functions included.

The end of the create is heard while the node waits for the end of the child Job:

| End of the create | What the node does |
|---|---|
| delivered | nothing; it keeps waiting for the end and for the cancel view |
| `Refused` (also when the upload of that create was refused) | the call is settled; your response function gets `Outcome::Refusal`; no cancel is handed over |
| removed after a report | the call is settled; fault `Error::CreateRemoved`; no cancel is handed over |

If an end and a removal of the create both come, the one the node sees first settles the call and the other is ignored.

## Request function and `JobSpec`

The request function takes the state (`Arc<State<F>>`) and gives a `JobSpec` or an error. It may be async and cannot change the state. Its error is yours: no call is made and the event `Failed` carries it unchanged.

`JobSpec::new(payload)` takes one of three forms, and `with_config(value)` adds the configuration:

| `Payload` | What the node hands over |
|---|---|
| `Inline(content)` | one create with the content inline |
| `Kept(content)` | the upload of the content, and then the create with the same Job id |
| `Reference { service, document_id }` | one create that names a document of another service |

The runner type is a `SubjectSegment`, made with `SubjectSegment::runner_type("analyst")`; a value that the wire does not accept cannot be made. `with_max_attempts(n)` passes the count of Runs that jobs may make to the create as given; it is not a retry of the node. A runner type that jobs refuses gives `Outcome::Refusal`.

## Outcome and response function

The response function takes the state and the `Outcome`, may be async, and gives an update or an error.

| `Outcome` | When |
|---|---|
| `Completed { inline, result_ref }` | the child Job completed, with the Result inline or a Result reference |
| `Failed { report, message }` | the child Job failed |
| `Cancelled { message }` | the child Job was cancelled |
| `Refusal(Refusal)` | the create ended `Refused`: the `reason` and the `message` of jobs. It is not the event `Refused` |

The library never decodes or fetches a Result, and never decides what a failure or a refusal means. A Result reference passes on as it came, and an inline Result is given in the form of `br-runner-jobs`; either may go in the state, and a later node reads a Result reference with the caller handle. You may turn a failed end into an update (an item of a map-reduce marked failed) or into an error. An update goes in the event `Result`, whatever the Outcome was; an error goes in the event `Failed`, unchanged.

## Caller source

The caller handle depends on the instance, as one graph serves many Runs. A caller source is an async function from the `InstanceId` to a `CallerHandle` or an error (`CallerSource`; any `Fn(InstanceId) -> impl Future<Output = Result<CallerHandle, Cause>>` is one). It is asked once for each instance. The host makes it.

## Faults

A fault fails the instance with the event `Failed`, carrying an `Error`. The error holds the error of the caller source or of `br-runner-jobs` as its source and names no error type of `br-runner-jobs`.

| `Error` | Cause | Cancel handed over |
|---|---|---|
| `CallerSource` | the caller source gave an error | no |
| `Upload` | the caller handle could not take the upload | no |
| `Create` | the caller handle could not take the create | no |
| `CreateRemoved` | the create was removed after a report (holds the error of the queue as its source when it has one) | no |
| `EndWait` | the end wait gave an error | yes, once, before the event |
| `UnknownEnd` | the end of the child Job is of a kind this version does not know | no |

An error of your own function is not a fault.

## Cancel

The node hands over a cancel of the child Job, once, as Requester, only when the create was handed over and the sync call is not settled, and then only when the cancel view fires (the event `Cancelled` follows) or the end wait gives an error (the event `Failed` follows). The cancel view fires also while the end of the create is pending.

- Before the create is handed over, the cancel view gives the event `Cancelled` and a fault gives the event `Failed`, with no cancel.
- Once the sync call is settled (an end in hand, a refused create, or a create removed after a report), no cancel is handed over. A cancel view while your response function waits ends that wait and gives the event `Cancelled`.
- If two errors come together, or an error and the cancel view come together, the one the node sees first gives the event.
- A finish or a cancel that cannot be handed over changes nothing; the node does not read the end of either. No finish is handed over after the cancel view.
- Whenever the instance ends before an end is in hand, the node abandons the end wait. A dropped instance hands over nothing more. Cancel is best effort.

## Ids and identity

The node makes the child Job id and the command ids as UUIDv7, new for each instance. After a restart the graph runs again from its start, and this node makes a new child Job with a new id. The node never sees, sets or stores the Passport, the parent Job or the Initial Job: the caller handle carries them.

## In a map-reduce

Put a `RunnerAsNode` as the internal node of the map-reduce: each element gets an instance, and each instance is one child Job. A failed runner is an item result: the response function turns the `Outcome::Failed` into an update that marks the item failed. See the known gap at the top of this file.

## Testing your node

The `test-double` feature of `br-runner-jobs` gives `memory_caller()`, a caller handle with a script: answer each write, give an end for a child Job id, and read the recorded calls. The child Job id is made by the node, so read it from the recorded create and then give the end:

```rust,ignore
let (handle, script) = memory_caller();
// build the node with a caller source that gives `handle`, start an instance, give an input
let create = /* the Create call in script.calls() */;
script.end(create.job_id, Answer::Now(SyncEnd::Completed { inline: Some(content), result_ref: None }));
```

## What the crate does not do

It does not decode or fetch a Result, decide what a failure or a refusal means, make the caller source, set a time limit for the end, retry, or keep anything from one instance to the next. A time limit is the cancel view, triggered by the host. What to put in the state, and how to avoid a second child Job after a restart, is yours.
