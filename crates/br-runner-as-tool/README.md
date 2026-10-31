# br-runner-as-tool

A runner type as a tool. It gives `RunnerTool`: you build it with the runner type, the tool name, the tool description, the argument schema, the tool mode and the caller handle of `br-runner-jobs`, and it gives a `Tool` of `br-agentic-contract`. An agentic node that calls this tool makes the other runner do a piece of work and gets its answer. The crate is on the calling side only: the called runner needs nothing from it, and the crate imports no runner and reads no registry.

The calling runner is the runner whose agentic node calls the tool. The called runner is the runner type that the tool makes work. "Tool" always means what the model calls; "runner" always means an image and a pod.

## Install

```toml
br-runner-as-tool = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-as-tool", tag = "br-runner-as-tool/v0.1.0", version = "0.1.0" }
br-runner-jobs = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-jobs", tag = "br-runner-jobs/v0.1.0", version = "0.1.0" }
br-agentic-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-contract", tag = "br-agentic-contract/v0.2.0", version = "0.2.0" }
br-llm-clients-core = { git = "https://github.com/BotResources/br-llm-clients-rs", package = "br-llm-clients-core", tag = "br-llm-clients-core/v0.5.0", version = "0.5.0" }
br-llm-messages = { git = "https://github.com/BotResources/br-llm-messages-rs", package = "br-llm-messages", tag = "br-llm-messages/v2.1.0", version = "2.1.0" }
uuid = { version = "1", features = ["v7"] }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pins above are the ones this crate is built with. For tests and examples, `br-runner-jobs` has the feature `test-double`, which gives an in-memory caller handle (`br_runner_jobs::test_double::memory_caller`) that you script to answer late, never, with an error or with a given end.

## Quickstart

This is `examples/sync_tool.rs`. It builds a `Tool` that makes the runner type `analyst` answer a question, in `Sync` mode. List the `Tool` in the tools port of the calling runner. Build one `Tool` for each run of the calling runner, with the caller handle of that run.

```rust,no_run
use br_agentic_contract::Tool;
use br_llm_clients_core::{Parameter, ParameterKind, Parameters};
use br_llm_messages::ToolName;
use br_runner_as_tool::{RunnerTool, ToolMode};
use br_runner_jobs::CallerHandle;

pub fn analyst_tool(caller: CallerHandle) -> Result<Tool, Box<dyn std::error::Error>> {
    let question = Parameter::new(
        "question",
        "What the analyst must find out.",
        true,
        ParameterKind::String,
    )?;
    let tool = RunnerTool::new(
        "analyst",
        ToolName::new("ask_analyst")?,
        "Asks the analyst runner a question and waits for its answer.",
        Parameters::new(vec![question])?,
        ToolMode::Sync,
        caller,
    )
    .build()?;
    Ok(tool)
}

fn main() {
    println!("analyst_tool(caller_handle) gives the Tool to list in the tools port");
}
```

The other two examples are in `examples/` and run in the tests: `background_tool` (a `Background` tool with a payload kept by jobs) and `builder_and_processor` (a tool with a payload builder and a result processor).

## Surface

| Item | Role |
|---|---|
| `RunnerTool::new(runner_type, name, description, parameters, mode, caller)` | The required inputs. `name` is a `ToolName`, `parameters` are the `Parameters` of `br-llm-clients-core`, `caller` is the `CallerHandle` of the run. |
| `.config(value)`, `.max_attempts(count)` | The configuration of the child Job and the count of Runs that jobs may make for it. Both go to the create as given. |
| `.payload_kind(kind)`, `.keep_result(flag)` | `PayloadKind::Inline` (default) or `PayloadKind::Kept`; the keep flag of the Result read (default off). |
| `.payload_builder(function)`, `.result_processor(function)` | The two async functions of the author. |
| `.build()` | Gives the `Tool`, or a `RunnerToolError`. |
| `ToolMode` | `Sync`, or `Background { caller_address }` with the mail address of the calling agent. |
| `Payload`, `AuthorFailure` | What the payload builder gives (`Payload::Content` or `Payload::Reference`), and how an author function says no: `AuthorFailure::refused(text)` or `AuthorFailure::fault(error)`. |
| `RunnerToolError`, `CreateRemoved`, `UnknownEnd` | The build error, and the two causes of a fault that this crate makes. |

## Rules the code keeps

**Building**

- The build checks the runner type with the rule that `br-runner-jobs` applies to the runner type of its own client (not empty, at most 64 characters, letters, digits, `_` and `-`, starting with a letter or a digit); a runner type that fails is `RunnerToolError::RunnerType` with `Error::InvalidInput`. The `ToolSpec` is made from the name, the description and the schema; its error is `RunnerToolError::Spec`, as `br-llm-clients-core` gives it. The crate adds no text to these errors and no other rule on the name, the description or the schema. When the build fails no `Tool` exists.
- A `Tool` is tied to the caller handle and to the tool mode it was built with, so to one run. Build it for each instance and do not keep it from one run to another. It keeps nothing from one call to the next and changes no state of the run. Several calls of one `Tool` run at the same time; each one has its own child Job.
- The crate sets no priority (a child Job takes the priority of its parent), no timeout, no depth limit and no retry. It hands each write over once. The delivery queue behind the caller handle sends a write again until jobs answers; that is not a retry of this crate.

**Tool mode**

- `Sync`: any runner type can be called. The call lasts as long as the child Job.
- `Background`: the child Job is created with the address, and the call returns right after the create is handed over. The answer comes by mail and by notices, which reach the calling agent through its inbox and not through this crate. Use it for a calling node that has an inbox, and for a called runner that has exactly one node with an inbox. The crate cannot see inside another image and does not check this.

**Payload**

- By default the payload is the arguments of the call as JSON, as received, in one message. The crate does not check the arguments against the schema.
- The payload builder gives the whole payload from the arguments, in place of the default; it is not merged with the arguments. Whatever else it needs, capture it when you build the tool. It can refuse the call with a text: the call then ends with an error flag that holds the text, and nothing is created or uploaded.
- `PayloadKind::Kept`: the upload of the payload is handed over under the child Job id, then the create with the same id. The crate waits for no end of the upload: a refused upload comes as a refused create, and a removed upload as a removed create. A `Payload::Reference` from the builder replaces the form, and the crate fetches nothing from it.
- The crate adds no timeout and no limit to the two author functions. When the cancel view triggers while one of them runs, the call ends as in Cancel.

**Sync call, in this order**

1. Run the payload builder, if set.
2. Mint the child Job id (a UUIDv7; a new command id is minted for each command).
3. Open the end wait for the child Job id (local, no reply, cannot be refused).
4. Hand over the upload, if the payload is kept.
5. Hand over the create in sync mode.
6. Wait together for the end of the create and the end of the sync call. The end of the sync call settles the call. A create that ends `Refused` ends the call with an error flag; a create that is removed ends it with a fault; a create that is delivered changes nothing.
7. On "completed": take the Result (inline when the end holds it, else read it with the reference and the keep flag), hand over the finish of the child Job whatever the Result read gave, then run the result processor, if set.
8. Give the tool result.

- The crate sends nothing to the result stream.
- The default content of a completed call is the message of the Result in one text block, as it is. A Result that has attachments gives one more text block for each attachment, headed by its name and type. A Result with no message and no attachment (and a completed end with no Result) gives one text block that says the runner returned nothing.
- The result processor is given the `Content` of the Result and gives content blocks or an `AuthorFailure`. Its blocks replace the default content.
- On "failed" or "cancelled" the child Job is over: nothing is read and nothing is finished.

**Background call**

- Run the payload builder if set, mint the child Job id, hand over the upload if the payload is kept, hand over the create in background mode, give the acknowledgement. After the builder the call waits for nothing and reads nothing from jobs.
- The acknowledgement has no error flag. Its text names the runner type and the child Job id, says that the create was handed over to jobs and is not confirmed until the notice "accepted" reaches the agent, and says that the notices name the child Job.
- The crate does not finish the child Job of a background call and does not cancel it after the call returned.
- The result processor and the keep flag have no effect in this mode.

**Tool result**

| Step | Outcome | What the call gives |
|---|---|---|
| Payload builder | refused | error flag with the text of the builder |
| Sync create | `Refused` (also when its upload was refused) | error flag with the text of jobs unchanged (the message of the refusal, or its reason when the message is empty) |
| Sync create | removed (also when its upload was removed) | fault |
| Upload or create, both modes | not taken by the caller handle | fault |
| Background create | taken by the caller handle | acknowledgement |
| End of the sync call | completed | the content of the Result, no error flag |
| End of the sync call | failed or cancelled | error flag; a first line names the runner type and the outcome, then the plain message of jobs unchanged |
| End of the sync call | any error of the wait | fault |
| Result read | `Refused` | error flag naming the Result read, with the text of jobs |
| Result read | any other error | fault |
| Result processor | an `AuthorFailure::refused` | error flag with that text |
| Payload builder, result processor | an `AuthorFailure::fault` | fault with that error as the cause |
| Finish, cancel | any outcome or none | ignored |
| Any step | the cancel view triggered | error flag that says the call was cancelled |

- An error flag is for an outcome that jobs or the called runner decided, or that an author function refused. A fault is for an outcome that is unknown or comes from the infrastructure; its cause is the error that ended the call, with no text added. A failed or cancelled child Job is never a fault. A slow child Job is never an error: the crate measures no time.
- A text this crate writes names no Job of the calling run and no Passport. A text that is empty or only white space is replaced by a short fixed text.

**Waiting and slowness**

- A call blocks only at these points: the payload builder; in sync mode the wait for the end of the create and of the sync call (one point), the Result read and the result processor. Every one of them can be left by the cancel view. A call waits for no other call.
- The finish and the cancel are handed over and never awaited; their answers are never read. A finish or a cancel that cannot be handed over is ignored.

**Cancel**

- The create counts as handed over from the moment the caller handle takes it.
- When the cancel view triggers before the create is handed over, the call ends at once and sends no create and no cancel. An upload already handed over stays in jobs.
- When it triggers after the create is handed over and before the call returned, the call ends at once and hands over one cancel for the child Job id, as the requester, with a new command id, and waits for no answer. No cancel goes for a create that ended `Refused` or was removed.
- A fault after the create was handed over and before the end of the sync call arrived hands over one cancel in the same way, except for a create that ended `Refused` or was removed. No cancel goes for a fault before the create, for a create the handle did not take, or after the end of the sync call arrived.
- When the end of the sync call has arrived, a cancel view that triggers sends no cancel. No finish is handed over after the trigger; a finish already handed over stays.
- In background mode, once the call has returned, the tool cancels nothing. The cancel of a whole tree of Jobs is cascaded by jobs.

**Identity**

- The crate never takes, reads, stores or logs the Passport. The caller handle fills the parent Job, the Initial Job and the Passport of the child Job from its own Work; this crate cannot set them. The only identity it holds is the mail address of a `Background` tool.

## Limits

- The crate does not know whether jobs accepted an upload or a create in background mode.
- The Result of the called runner is the `Content` that `br-runner-jobs` gives (a message and attachments). A JSON value is given as the JSON text it arrives as.
- The runner type is checked with the segment rule of the jobs wire at build time; `br-runner-jobs` has no runner type value of its own.
- An end of the sync call of a kind this crate does not know is a fault (`UnknownEnd`).
- A create removed from the queue without a refusal (`DependsOnRemoved`, `LeftUnsent`) is a fault whose cause is `CreateRemoved`.
- A `Tool` used after its run ended gives what the caller handle gives; the crate does not check it.
- The tests and the examples use the in-memory caller side of `br-runner-jobs`: no bus, no GraphQL.
