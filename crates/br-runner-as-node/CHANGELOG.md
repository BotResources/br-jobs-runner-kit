# Changelog

## 0.1.0

Released 2026-10-31.

First release. `RunnerAsNode`: a node of `br-graph-contract` that makes another runner do one step of a graph, by a sync child Job of the jobs service.

### Added

- `RunnerAsNode<F>`: kind `programmatic`, subkind `runner_call`, generic over the fields of its graph. `RunnerAsNode::new(name, runner_type, request, response, caller_source)` cannot fail and keeps nothing from one instance to the next; `with_max_attempts(n)` passes the count of Runs that jobs may make to the create as given.
- `SubjectSegment` (re-exported), the validated runner type, made with `SubjectSegment::runner_type(..)`.
- `JobSpec` and `Payload`: the request function gives the payload (`Inline`, `Kept` or `Reference`) and the configuration. A kept payload is handed over as an upload before the create with the same Job id.
- `Outcome` (`Completed`, `Failed`, `Cancelled`, `Refusal`) and `Refusal`: what the response function gets. The library never decodes or fetches a Result.
- `CallerSource` and `CallerFuture`: an async function from the instance id to the `CallerHandle` of `br-runner-jobs`; any matching closure is one.
- `Error` with a case for each fault: `CallerSource`, `Upload`, `Create`, `CreateRemoved`, `EndWait`, `UnknownEnd`. It holds the cause as its source and names no error type of `br-runner-jobs`.
- Example `call_a_runner_as_node`, the quickstart of the README, run by the tests against an in-memory caller handle and a real delivery queue over a scripted jobs service.

### Behavior

- One instance does one step and sends one event: it closes its input at the first input and uses the last state it was given; it awaits the request function, asks the caller source, opens the end wait, hands over the upload (kept payload) and the create, waits for the end, hands over the finish when the end is completed, awaits the response function and sends the event `Result` or `Failed`. It never sends the event `Refused`.
- The node blocks only on the caller source, the end and the two functions; the cancel view ends every one of those waits. It adds no timeout and no retry count and sends nothing again.
- A create that ends `Refused` (also for a refused upload) gives `Outcome::Refusal`; a create removed after a report is the fault `CreateRemoved`. Neither hands over a cancel.
- A cancel of the child Job is handed over once, after the create, when the cancel view fires or the end wait gives an error, and only while the sync call is not settled. No finish is handed over after the cancel view. A dropped instance hands over nothing more.
- The child Job id and the command ids are UUIDv7, new for each instance.
- Known gap: in a map-reduce, an instance waits on its caller source until the host triggers the cancel view, as long as the host cannot make a caller handle for the instances that a map-reduce creates itself.
