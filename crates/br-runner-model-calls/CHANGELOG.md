# Changelog

## 0.1.0

The envelope around any model client of a runner: one model call record per call, published on the bus as the integration command of a consuming service. Registry: library `br-runner-model-calls`, minor 0.1.

### Added

- `ModelCallContext` (`new`, `with_user`, `with_label`): the job id, run id, attempt and initial job id of the trigger, the user the work runs for, and labels.
- `RecordCommand`: the seam through which the host gives the coordinates and the `IntegrationCommand` of the consuming contract for a `ModelCallRecord`. The library depends on no contract of a consuming service.
- `PublishSettings` (default 3 attempts, a wait of 2 s, a first pause of 200 ms that doubles) and `SettingsError` (zero attempts, zero wait).
- `ModelCallRecorder` (`new`, `wrap`, `published`, `failed_publishes`, `flush`): publishes through the `Fabric` of `br-util-nats-fabric` with the call id as the message id, each attempt bounded by the wait; a failure is a warning and a counter, never a panic and never a failure of the model call.
- `MeteredClient`: a `Client` of `br-llm-clients-core` 0.5 (deliberately not an `ObservedClient`) with `endpoint()`. A UUIDv7 call id per call, deltas and outcome passed through unchanged, the record handed to a task of its own; a call dropped by a timeout or a cancellation is recorded as abandoned when a usage is known, and not at all otherwise.
- `ModelCallRecord`: ids, labels, endpoint, requested and answered model, raw usage and common `Usage`, the context facts `service_tier`, `inference_geo`, `speed` and the provider cost read from the top level of the raw usage, `started_at` in UTC, duration, time to first token, success and abandoned flags.
- Re-exports of `ObservedClient`, `RawUsageCell` and `CallEndpoint` of `br-llm-clients-common-call` 0.3.
