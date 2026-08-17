# br-jobs-runner-core

Pure state machines for a [svc-jobs](https://github.com/BotResources/svc-jobs)
runner. No I/O, no NATS, no async — the transport lives in
`br-jobs-runner-kit`, which drives these machines.

## Surface

- `PresenceSession` — the presence lifecycle: publish READY immediately,
  heartbeat at a configured interval (validated strictly below the presence
  TTL), `begin_drain()` forces an immediate DRAINING publish, `close()` yields
  the final key delete. `poll(now)` returns the action due, `next_due()` the
  deadline to sleep until.
- `SlotPool` — run-slot accounting: `try_claim` / `release` over run ids,
  capacity-bounded, `begin_drain()` refuses new claims while in-flight runs
  finish. `has_free_slot()` is the gate for pulling the next trigger.
- `CancelSet` — fold of the cancel KV bucket: `apply_put` / `apply_delete`
  from watch events, `replace_with_replay` on (re)connect,
  `is_cancelled(run_id)`.
- `Backoff` — deterministic capped exponential backoff for reconnects.

| Thing | Why it is the way it is |
|---|---|
| Time passed in (`poll(now)`, `next_due()`), never read | Machines stay pure and deterministic under test; the harness owns the clock and the sleep. |
| `begin_drain()` resets the heartbeat | DRAINING must reach the server immediately, not at the next scheduled beat, so the server stops routing triggers to a stopping instance. |
| `CancelSet::replace_with_replay` swaps the whole set | A reconnect cannot trust deltas — the full bucket replay is the only honest state; per-run stickiness lives in the harness's already-fired cancellation tokens. |
