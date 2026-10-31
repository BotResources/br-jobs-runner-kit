# br-runner-toolkit-chart

A Helm library chart. It holds the values of the settings of `br-runner-toolkit` at the level of the runner and one named template that renders them as the environment of the container of a runner. It is a library chart: it renders nothing by itself.

## Use

The thin chart of a runner declares this chart as a dependency with the alias `runner`, sets the eight values under `runner`, and includes the named template in the `env` of its container.

```yaml
dependencies:
  - name: br-runner-toolkit-chart
    alias: runner
    version: "0.1.0"
    repository: "oci://ghcr.io/botresources/charts"
```

```yaml
runner:
  runner_version: "1.0.0"
  max_concurrent_runs: 4
  ack_wait: 30000
  drain_timeout: 60000
  cancel_grace: 5000
  claim_wait: 10000
  outcome_wait: 10000
  retry_wait: 1000
```

```yaml
env:
  {{- include "br-runner-toolkit-chart.env" . | nindent 2 }}
```

## Values

Each value is required and has no default. The name of a value is the name of the setting of `br-runner-toolkit`. A duration is an integer in milliseconds.

| Value | Environment variable | Rule |
|---|---|---|
| `runner_version` | `RUNNER_VERSION` | text, not empty |
| `max_concurrent_runs` | `MAX_CONCURRENT_RUNS` | integer, 1 or more |
| `ack_wait` | `ACK_WAIT_MS` | milliseconds, 1 or more |
| `drain_timeout` | `DRAIN_TIMEOUT_MS` | milliseconds, 0 or more |
| `cancel_grace` | `CANCEL_GRACE_MS` | milliseconds, 0 or more |
| `claim_wait` | `CLAIM_WAIT_MS` | milliseconds, 1 or more |
| `outcome_wait` | `OUTCOME_WAIT_MS` | milliseconds, 1 or more |
| `retry_wait` | `RETRY_WAIT_MS` | milliseconds, 1 or more |

The library has no reader of these variables. The runner reads them and gives them to `Settings::builder()`.

A missing value, a value of the wrong type, a value out of range and an unknown value fail the render. A thin chart that does not declare the alias `runner` fails the render too.

## Why

| Thing | Why it is the way it is |
|---|---|
| The alias `runner` | Helm checks `values.schema.json` of a library chart against the values of its own scope, which is the key of the alias. Without the alias, the schema would not check the values of the runner. |
| A duration in milliseconds | One unit for every duration, and an integer needs no parsing. |
