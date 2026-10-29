# br-runner-toolkit

The library every runner binary imports to be a runner. It gives the agentic nodes of a graph the ports of `br-agentic-contract` (agent port, thread access) over `br-runner-threads`, and it holds the failure, outcome, saved-state and settings rules of a run over `br-graph-executor`.

This 0.1.0 is not complete: the runner builder, the run loop, the inbox and the test host need `br-runner-jobs`, which is not released yet.

## Install

```toml
br-runner-toolkit = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-toolkit", tag = "br-runner-toolkit/v0.1.0", version = "0.1.0" }
```

## Surface

| Item | Role |
|---|---|
| `PortSets`, `PortSetRequest`, `PortSet` | Ask for the ports of an agentic node by node name, then bind them to a `ThreadsClient`. |
| `ToolkitAgentPort` | The agent port. One UUIDv7 identity per agent key and run, registered before it is returned. |
| `ToolkitThreadAccess` | The thread access over `br-runner-threads`. |
| `RunHandle` | One run in flight: its observer for the executor, its agent map, its end. |
| `SavedState`, `SavedAgent` | The saved state document: the fields and the agent map. |
| `FailureReport`, `FailureMapping`, `StartRefusal`, `terminal_for`, `Terminal` | The failure and outcome rules. |
| `ToolkitSettings` | The settings of the toolkit that do not belong to a client. |
| `PortError`, `PortErrorCode` | The error of the ports. |
| `AgentRegistrar`, `AgentRegistration` | The call that registers an agent with jobs. |
