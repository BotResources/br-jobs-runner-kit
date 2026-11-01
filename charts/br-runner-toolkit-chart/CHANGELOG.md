# Changelog

## 0.1.0

First release. A library chart with one named template, `br-runner-toolkit-chart.env`, that renders the eight settings of `br-runner-toolkit` as the environment of the container of a runner.

- The thin chart of a runner declares this chart as a dependency with `alias: runner` and sets the eight values under `runner`.
- Each value is required. The chart gives no default, and `values.schema.json` rejects a missing value, a value of the wrong type and a value out of range.
- A duration is an integer in milliseconds.
