# Sandbox battery: Rust tests

Tests that run the released `br-runner-sandbox` adapter (tag `br-runner-sandbox/v0.1.3`) against the live test cluster, the real object store and the real tools image. This is a separate cargo project (own `[workspace]`), not a member of the kit workspace. It uses the released harness crates at 0.2.0 from `br-llm-graph-rs`.

No test is ignored. A test that cannot reach the cluster, the store or a named image fails with a message that names the missing thing.

## Run

```sh
cd tests/sandbox-battery/rust
set -a
: "${WORK:=$HOME/sbx-battery-work/battery}"
. ../names.env
. ../scenarios/names-chart.env
. <creds.env of the set-up>
set +a
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Tests that change the cluster (scale the pool, stop the keeper, cordon the node, change helm values) restore it when they end, also on failure. The restore of the helm values also runs when the test process gets SIGTERM or SIGINT: the guard registers its restore in a list (`src/restore/`, one list for the process; each test makes its own list), a thread of the process waits for the two signals, runs every registered restore once and exits with 143 (SIGTERM) or 130 (SIGINT); the restore of the guard that ends normally and the one of the signal never run twice. A signal that arrives while a `helm upgrade` of `set` runs waits for that upgrade to end before the restore starts, so two `helm` commands never run together, and no `set` starts after the restore began. The other guards (keeper scale, node cordon, pod reaper) are not registered: a kill signal (SIGKILL) or a crash of the process leaves them as they are, and so does SIGTERM for them. Unit tests of the signal path and of the guard (with a fake helm, so no `helm` runs): `cargo test --lib restore` and `cargo test --lib value_guard`. On chart 0.4.0 and later the pool is a `SandboxPool` resource, not a chart value: the tests that change it use `PoolGuard` (`src/pool_guard/`). It reads the `spec` of the object named `SANDBOX_POOL_NAME` in the namespace `NS_PREFIX-CLIENT_A-NET_NONE` (and nothing else: it never creates the object, never lists, never uses another namespace), changes the keys that the test names with one `kubectl patch --type merge`, and puts back the whole `spec` it read with one `kubectl patch --type json` when it drops, when the test fails or panics, and on SIGTERM or SIGINT (the same registry as the helm guard, same rules: one restore, none while a change runs, no change after the restore began). It sets `spec.minReplicas` (the old `pool.floor`) and raises `spec.maxReplicas` in the same patch when it is below the floor. Unit tests (a fake `kubectl` script, so no cluster is touched): `cargo test --lib pool_guard`. The namespace is the one that the variables name: use a namespace made for the test, never one that other users run on. A restore that fails is written to `checks.tsv` as a failed check and fails the test. Timed tests take the lock `TIMING_LOCK` (a `mkdir` lock, the same one as the shell scripts), so they never overlap with a shell scenario. Run one at a time: `--test-threads=1`. The lock folder holds a file `pid` with the process id of the holder. When the holder is gone (`kill -0` fails with "No such process"), the next test takes the lock over; a live process, also one of another user ("Operation not permitted"), is never taken over. Limit: a lock folder with no `pid` file (a holder that stopped between the folder and the file, or a lock made by a shell scenario) is never taken over and must be removed by hand; a reused pid of a dead holder also counts as alive. The wait for the lock ends with an error after six hours. Unit tests of the take-over: `cargo test --lib lock`.

The fault gates of `src/tunnel` (a local TCP gate between a client and the API or the store) change their setting with `Gate::set(mode).await`. A new connection is judged by the setting at its start; a connection that was open under the previous setting is dropped before `set` returns (for the settings `Refuse` and `Stall` as for every other one), except a manifest write that `PauseManifestWrite` holds, which goes on under the new setting. `set` returns an error when a connection of the previous setting is still open after 10 s. Unit tests: `cargo test --lib tunnel`.

The chart version, the adapter version and the keeper image that `seed_bound` writes in its measurements are read when the test runs: the chart version from `Chart.yaml` of `CHART_DIR`, which must hold the version `CHART_VERSION` (else the test fails and names both), the adapter version from the pin in `Cargo.toml`, the keeper image from the container of the keeper Deployment that is named `KEEPER_CONTAINER` (`src/versions.rs`).

Results go to `$EVIDENCE/checks.tsv` (name, pass, fail or skip, detail, time) and `$EVIDENCE/measurements.tsv` (name, value, unit, load 1 min, load 5 min, time), in the format of `../lib.sh`. A test ends with an error when one of its checks failed, when it recorded no check, or when an evidence file cannot be written, so the exit code of `cargo test` is the verdict. A check with the status `skip` is written for a test that does not apply to the chart line (see the map below): it is never a pass. The end of each test prints `summary: N pass, N fail, N skip` and, when nothing passed, `skipped, nothing passed`; `cargo test` itself shows such a test as `ok`, so read the `skip` rows (or the summary line with `--nocapture`) and never count a test that only skipped as proved. A test with no check and no skip fails.

No CI job of this repository compiles or runs this project: it is outside the kit workspace and its tests need a live cluster, a store and the images. The checks above (`cargo fmt --check`, `cargo clippy`, `cargo test`) are run by hand.

## Variables

All names are read in `src/env.rs` (one `Env` struct). A missing variable fails the test with the variable name and the file it comes from.

| Variable | Comes from |
|---|---|
| `CTX_TEST`, `NODE_TEST`, `NS_PREFIX`, `CLIENT_A`, `NET_NONE`, `RS_PREFIX`, `CHART_VERSION`, `KEEPER_LABEL` | `scenarios/names-chart.env` |
| `SANDBOX_CONTAINER`, `SEED_CONTAINER`, `SEED_PATH`, `AGENT_BSDTAR`, `SEED_BSDTAR`, `SANDBOX_PYTHON` | `scenarios/names-chart.env` |
| `HELM_RELEASE`, `HELM_NAMESPACE`, `SYSTEM_NS`, `KEEPER_DEPLOY`, `RUNNER_NS`, `RUNNER_SA` | `scenarios/names-chart.env` |
| `VALUES_IMAGE_KEY`, `WARM_MAX_AGE_VALUE_KEY` | `scenarios/names-chart.env` |
| `S3_ENDPOINT_HOST`, `S3_BUCKET`, `S3_REGION`, `S3_PATH_STYLE`, `S3_ACCESS_KEY`, `S3_SECRET_KEY` | `creds.env` |
| `WORK` or `BATTERY_WORK`, `EVIDENCE`, `TIMING_LOCK` | optional; defaults are the work folder `~/sbx-battery-work/battery`, `$WORK/evidence`, `$WORK/timing.lock.d`. Set `WORK` before `names-chart.env` is sourced: its defaults (`BATTERY_WORK`, `TIMING_LOCK`, `CHART_VALUES_FILE`) are computed when the file is sourced, and with `WORK` unset they become `/values-test.rendered.yaml` and the like |
| `CHART_VALUES_FILE` | a file of values for `helm template` of the chart of the release. `names-chart.env` gives `$BATTERY_WORK/values-test.rendered.yaml` (made by `../chart.sh render`), and the project has the same default when the variable is not set at all. The file must exist: the test that reads it fails with the path when it does not. Used by `expiry::a_save_age_of_zero_or_less_is_refused_by_the_chart_and_the_adapter`; the user values of the release (`helm get values`) are a good file |
| `SANDBOX_POOL_NAME` | optional, default `pool`; the name of the `SandboxPool` object that `PoolGuard` reads and changes in the namespace `NS_PREFIX-CLIENT_A-NET_NONE` (chart 0.4.0 and later; the object must exist before the test) |
| `CHART_DIR` | the folder of the chart of the installed release (the one that `CHART_VERSION` names). It is read for the chart version of the measurements (the test fails, with both versions, when the folder holds another chart version than `CHART_VERSION`, also when `CHART_DIR` is unset and the default folder is used) and it is the chart that the guard of the chart values (`ValueGuard`) upgrades and restores with (`PoolGuard` does not use it), so a test that uses `ValueGuard` fails before any change when `CHART_DIR` is not set, has no readable `Chart.yaml`, or holds another chart version than `CHART_VERSION`. Default when unset: the chart of this repository, which only the tests that do not change chart values may use |
| `POOL_ID_STRIP` | optional; replaces the choice below. It is the prefix that is removed from the namespace name to get the pool id (also an empty value); it must be a prefix of the namespace name and leave a name |

The pool id (pool label, adapter `pool_id`, ReplicaSet name `$RS_PREFIX<pool id>`) follows the chart version, as the shell scenarios do: below 0.4.0 it is the namespace name without the prefix `$NS_PREFIX-` (`{client}-{mode}`), from 0.4.0 on it is the namespace name (`{NS_PREFIX}-{client}-{mode}`). `CHART_VERSION` must be three numbers (a suffix such as `-rc1` is ignored). A test that stops the keeper waits until no pod with the label `KEEPER_LABEL` is left in `SYSTEM_NS` (at most 120 s, then the test fails with the label and the namespace) before it changes the pool, and the keeper scale guard puts the keeper back at the end.

## Namespace of the adapter tests

The adapter tests of this repository take the network mode from the suffix of the namespace name. This is a rule of the tests, not of the adapter or of the chart. They need a namespace named `<prefix>-<client>-<mode>` with the mode `none`, `allow` or `full`: `NS_PREFIX`, `CLIENT_A` and `NET_NONE` build the name, and `NET_NONE` is the mode. For example the namespace `sbx-pool-w-none` has `CLIENT_A` `pool-w` and `NET_NONE` `none`. A namespace whose last part is not a mode (for example `sbx-pool-a`) stops the tests before they measure anything.

The pool must be made in that namespace by one operator upgrade of the chart that lists the namespace, since the chart makes the namespace and its fence objects. The warm pods are named with the prefix `RS_PREFIX` (`warm-` in `../scenarios/names-chart.env`). A scratch namespace of the shell scripts is no target for these tests: it has no `br-sandbox-config` and no binding of the runner (see "Scratch pools" in `../scenarios/README.md`).

## Seen on a cluster

These results are from one cluster, with chart 0.4.0 as released and adapter 0.1.3, in a scratch namespace of mode none made by an operator upgrade. Each row is one run unless it says otherwise; no spread is known.

| Test | What was seen |
|---|---|
| `open_images::a_pod_that_never_gets_ready_gives_image_unavailable_at_the_open_timeout` | Passed. The open ended with the error `ImageUnavailable` at 10.1 s, for an open timeout of 10 s. |
| `open_images::a_pool_whose_image_fails_gives_image_failing_at_once` | Passed. The open ended with the error `ImageFailing` with the reason `ImagePullFailing` in 0.03 s; the reason is equal to the condition of the pool. |
| `seed_bound::open_with_the_stress_seed_and_no_bound_leaves_no_pod_after_it_ends_or_is_stopped` | Passed: 2 tries ended Ok and no pod was left. |
| `seed_bound::open_with_the_stress_seed_and_a_set_bound_ends_within_the_outer_bound_and_leaves_no_pod` (bound 120 s) | Two runs: one failed and one passed. In the failed run, tries 1 and 2 took 7.5 s and 7.7 s. In try 3 of 3 the seed exec did not end, and the adapter stopped it at the exec timeout, at 120.2 s, with the error that the seed did not end in the time that the configuration gives. No seed was written, the open failed and no pod was left. The second run passed (tries of 5.8 s, 7.7 s and 8.7 s). The cause of the stall is not known: it is a known open stall of the adapter on the copy-in of the seed. This test is not a plain pass on this cluster. |
| `expiry::with_the_keeper_stopped_an_old_save_is_expired_on_the_read_side` | Passed. An old save was expired on the read side; the test stops the keeper and puts it back to 2. |

The tests of this project that are not in this table were not run on this cluster with this chart and adapter, and their rows above and below say what the tests do, not what was seen.

## Which tests are for which chart line

`CHART_VERSION` picks the line (`src/chart_line.rs`): from 0.4.0 on the pool is a `SandboxPool` resource (the image, `minReplicas`, `maxReplicas`, `margin` and the size are in its `spec`; the chart refuses the values `image` and `clients`); before 0.4.0 they are chart values.

| Test | 0.4.0 and later | Before 0.4.0 |
|---|---|---|
| `start_times::fresh::fresh_start_with_a_seed_of_10_mb_is_within_500_ms` | runs; `PoolGuard` sets `spec.minReplicas` 3 | runs; `ValueGuard` sets `clients.{client}.modes.{mode}.pool.floor` |
| `start_times::ladder::restored_start_ladder_10_30_50_100_mb_with_stalls_and_split` | runs; `PoolGuard` sets `spec.minReplicas` 4 | runs; `ValueGuard`, floor 4 |
| `seed_bound` (2 tests) | run; `PoolGuard` sets `spec.minReplicas` 3 | run; `ValueGuard`, floor 3 |
| `expiry` (3 tests) | run; `ValueGuard` sets `save.ttl_seconds` (a release value) | run; same |
| `open_images::a_pool_whose_image_fails_gives_image_failing_at_once` | runs; `PoolGuard` sets `spec.image` and puts the spec back at the end and on SIGTERM or SIGINT. On chart 0.4.0 (adapter 0.1.3) it passed in a scratch namespace of mode none (see "Seen on a cluster"). If the pool admission refuses the failing image, the test fails with the refusal text; if only the pod creation is refused, the wait for the failing condition ends in an error after 300 s. In both cases the test fails and never passes | runs; `ValueGuard` sets the key `VALUES_IMAGE_KEY` |
| `images` (3 tests) | skip row `skip`: "not for chart 0.4.0: the image is set in the SandboxPool spec; see the README map" | run; `ValueGuard` sets the key `VALUES_IMAGE_KEY` |
| `open_images::a_pod_that_never_gets_ready_gives_image_unavailable_at_the_open_timeout` and every other test | run (no pool change); the result of this one on chart 0.4.0 is in "Seen on a cluster" | run |

On 0.4.0 and later `quota.extra_pods` is not changed by these tests: the quota of the namespace holds `limits.pods` plus `quota.extra_pods`, which is not tied to the floor any more. On chart 0.4.0 and later no test of this project changes the image of a pool except `open_images::a_pool_whose_image_fails_gives_image_failing_at_once`; the three tests in `images` skip with a skip row, and a skip is not a proof. The three skipped tests have no Rust replacement for 0.4.0. Two of their sentences are covered by shell scenarios of `../scenarios` on a scratch `SandboxPool`: a new push under the same tag (`s22-same-tag-push.sh`) and an image that cannot be pulled (`s23-image-fails-at-once.sh`, which reads the pool condition and the pod state; it opens no thread, so the adapter error of that case is covered by `open_images::a_pool_whose_image_fails_gives_image_failing_at_once`).

## Not run on chart 0.4.0

The chart refuses the key `image` of the values, so these three tests of `images` write a skip row and nothing else on chart 0.4.0 and later. They prove nothing there:

- `images::a_second_image_comes_by_a_change_of_the_image_value_only`
- `images::a_new_push_under_the_same_tag_reaches_the_pool_within_the_warm_max_age`
- `images::an_image_that_starts_and_crashes_at_once_gives_image_failing`

The image error `ImageFailing` by a crash at start is not run on chart 0.4.0. The last result of the crash test is from chart 0.2.4 (it passed in 23 s). The image error `ImageFailing` by a failing pull is run on chart 0.4.0 by `open_images` (see "Seen on a cluster").

Variables that only some tests need (the test fails with the variable name when it is not set):

| Variable | Used by | Meaning |
|---|---|---|
| `SECOND_IMAGE` | `images` | image reference for the second image, set by value |
| `FIRST_VERSION_COMMAND`, `SECOND_VERSION_COMMAND` | `images` | a command in the pod that prints the version of the first and of the second image |
| `MOVING_IMAGE`, `MOVING_PUSH` | `images` | the image reference of a tag that is pushed again, as the pool pulls it (`MOVING_IMAGE`), and the image reference that the test builds and pushes to the registry as the host reaches it (`MOVING_PUSH`). The pull policy of the pool must be `Always` for this image |
| `BASE_IMAGE` | `images` | base image used to build the moving image |
| `CRASHING_IMAGE` | `images` | an image that starts and ends at once |
| `BYTES_SERIES`, `BYTES_ITERATIONS` | `bytes_under_load` | label printed on each line of the series (default `unlabelled`) and number of iterations (default 10; a value that is not a whole number of 1 or more fails the test, and a series with no `ok` line fails the check) |
| `LBL_MANAGED` | `adapter_document` | label (from `scenarios/names-chart.env`) that marks the namespaces of a sandbox pool; the test builds the adapter configuration from the document of each of them |
| `TLS_TRUSTED_ENDPOINT` | `store_client` | optional, https endpoint with a public root, default `https://s3.amazonaws.com` |

## Coverage

| Sentence or check | Test |
|---|---|
| Conditional create refuses a second writer | `store_client::conditional_create_refuses_a_second_writer` |
| Multipart size and abort | `store_client::multipart_part_has_its_size_and_abort_leaves_nothing` |
| List, delete, path style | `store_client::list_delete_and_path_style_work_on_the_real_store` |
| System roots only | `store_client::store_client_trusts_system_roots_only` |
| A read of a 50 MB file and a foreground run with 20 MB on stdout or on stderr never lose bytes without an error (lines `ok`, `error-reported`, `LOSS-WITHOUT-ERROR`) | `bytes_under_load::a_read_of_a_50_mb_file_never_loses_bytes_without_an_error`, `bytes_under_load::a_foreground_run_never_loses_output_bytes_without_an_error` |
| Exec: stdin close, true exit code | `exec_layer::process::exec_gives_true_exit_codes_and_ends_stdin_on_request` |
| Stream with no status is a fault | `exec_layer::stream::a_stream_with_no_status_is_a_fault` |
| Pod watch: changes and snapshot after a break | `open_cluster::the_warm_pod_watch_survives_a_break_of_the_api_connection`, `open_release::the_watch_follows_deleted_and_new_warm_pods` |
| Stale patch is a conflict | `open_pool::two_opens_that_race_for_one_warm_pod_give_it_to_one_only` |
| Cold pod from the template | `open_pool::a_cold_pod_from_the_template_starts_like_a_warm_pod_and_stays_out_of_the_replica_set` |
| Pool condition read as it is | `open_images::a_pool_whose_image_fails_gives_image_failing_at_once` |
| bsdtar argument lists | `exec_layer::tools::bsdtar_takes_the_argument_lists_of_the_crate` |
| Save then restore: files, modes, owners, dirs, times; a file written through the port belongs to the agent user (a file of another owner is not made: the seed rows cover root-owned files) | `save_restore::save_then_restore_gives_back_files_modes_owners_dirs_and_times`, `save_restore::a_file_written_through_the_port_belongs_to_the_agent_user` |
| Save of a missing scope path | `save_restore::a_save_of_a_missing_scope_path_is_a_fault_with_the_record_failed` |
| Background command and its id | `exec_layer::process::a_background_command_survives_and_its_id_is_right` |
| Scripts of the exec layer | `exec_layer::tools::the_scripts_of_the_exec_layer_work_in_the_agent_image_shell` |
| Exec timeout (the call returns a fault near the limit; whether the command still runs in the pod is recorded, not asserted) and save timeout | `exec_layer::process::a_command_over_exec_timeout_returns_a_fault_near_the_limit`, `save_restore::a_save_over_save_timeout_stops_with_the_record_timeout` |
| Seed root-owned and read-only | `save_restore::the_seed_is_root_owned_and_read_only_for_the_agent` |
| No TLS crypto provider set | `crypto_provider` (own test binary, fresh process) |
| Two opens race for one warm pod | `open_pool::two_opens_that_race_for_one_warm_pod_give_it_to_one_only` |
| Claim moves the pod out of the ReplicaSet | `open_pool::a_claim_moves_the_pod_out_of_the_replica_set_and_a_new_warm_pod_appears` |
| `ImageFailing` at once | `open_images::a_pool_whose_image_fails_gives_image_failing_at_once` |
| `ImageUnavailable` at the open timeout | `open_images::a_pod_that_never_gets_ready_gives_image_unavailable_at_the_open_timeout` |
| Live pod reused, idle deletion | `open_cluster::a_live_pod_is_reused_with_its_files_and_kept_only_for_the_idle_time` |
| Release: small deleted, large kept | `open_release::a_release_deletes_a_small_box_and_keeps_a_large_saved_one_idle` |
| Dropped open | `open_drop_erase::a_dropped_open_leaves_no_pod_and_no_instance` |
| Erase | `open_drop_erase::an_erase_of_a_thread_leaves_nothing_in_the_store_and_the_cluster` |
| Only the rights of the intent | `open_release::the_adapter_works_with_only_the_rights_of_the_intent` |
| Slow or lost kube API gives a fault, not a stuck call | `open_cluster::a_slow_or_lost_kube_api_gives_a_fault_not_a_stuck_call` |
| Fresh start within 500 ms with the seed (and the stress seed: limit reported, at least one sample) | `start_times::fresh::fresh_start_with_a_seed_of_10_mb_is_within_500_ms` |
| Save, failed turn, tree equal, go back | `failed_turns`, `tree_equal`, `go_back`, `save_keys`, `missing_save`, `crash_save` |
| Expiry after 60 s, named start results | `expiry`, `start_results` |
| Second image by value, a new push under the same tag reaches a new warm pod (no box is opened while waiting, so the pool is not renewed by claims), two image errors. On chart 0.4.0 and later only `open_images` runs here; for the push, `../scenarios/s22-same-tag-push.sh` measures the creation time of the new warm pod against the creation time of a marker object that it makes in the scratch namespace right after the push (both stamped by the API server, so the marker is a bound of a few seconds on the push time), at most `warm_max_age_seconds` and with no margin, and then checks that the image id of that pod is the pushed content within a wait limit (see "Not run on chart 0.4.0") | `images`, `open_images` |
| Harness suite with the adapter | `harness_suite`, `harness_open`, `harness_excluded` (see below) |
| Hard links, executable bit | `properties::hard_links_and_the_executable_bit_survive_save_and_restore` |
| A file that the agent deleted stays deleted after the next restore, and the file that stayed is still there. The image files in `/work` and `/home/agent` are hidden by the volumes of the chart, so the property is shown with two files that the agent writes itself (one under each path). An image file outside the save scope is not saved and is not restored (not tested here) | `properties::a_file_deleted_by_the_agent_stays_deleted_after_the_next_restore` |
| Restore ladder 10, 30, 50, 100 MB, stalls, cold start effect | `start_times::ladder::restored_start_ladder_10_30_50_100_mb_with_stalls_and_split`, `start_times::cold::cold_start_effect_of_the_init_container_and_the_image_size` |

## Harness suite with the adapter

The upstream tests of the harness crates drive `MemorySandbox` and read its test hooks (`script`, `fail_save`, `expire_save`, `saved`, `running`, `is_released`, and others) or wrap it in a rig that records the steps. The adapter has none of these hooks, so those tests cannot run unchanged. `harness_excluded.txt` lists each of them by its exact upstream test name, with the hooks that make it need the double (`harness_excluded` checks that every row has four fields and a reason, that no name is listed twice, and that each name is a test function of the named file in the upstream source at the revision pinned in `Cargo.lock`; it reads that source in the cargo git checkouts).

The upstream tests that are not listed in `harness_excluded.txt` are not run again against the adapter: nothing in this project includes or calls an upstream test. Those of them that use a sandbox are the unlisted tests of these files: in `br-agentic-harness-contract`, `canary.rs`, `dropped_open.rs`, `lifecycle_open.rs`, `open_faults.rs`, `rules.rs`, `sandbox.rs`, `save_choice.rs`, `save_rules.rs`, `start.rs`; in `br-agentic-harness`, `close.rs`, `credential.rs`, `rules.rs`. The check "the harness test suite passes with the adapter" is proved only by the replacement behaviours of the table below, which use a real `Harness` with the adapter behind a recording wrapper (`src/spy.rs`) and observe the cluster and the store.

| Upstream behaviour | Test with the adapter |
|---|---|
| Close `Completed`: stop commands, save, release; next turn restored | `harness_suite::a_completed_close_stops_the_commands_then_saves_then_releases_and_the_next_turn_restores` |
| Close `Failed` or `Cancelled`: stop commands, release, no save | `harness_suite::a_failed_or_cancelled_close_stops_the_commands_and_releases_and_never_saves` |
| A process that cannot be killed (an id that is not a process number: the adapter refuses it, while it takes a missing process number as killed) does not stop save and release, and the close returns the failure | `harness_suite::a_process_that_cannot_be_killed_does_not_stop_the_save_and_the_release` |
| `AlreadySaved` is tolerated | `harness_suite::a_box_already_saved_by_its_own_turn_does_not_fail_the_completed_close` |
| Close with no open box | `harness_suite::a_close_of_an_instance_with_no_open_box_is_a_success_and_touches_nothing` |
| An open instance is refused until its box is released | `harness_open::an_instance_with_an_open_box_is_refused_until_the_box_is_released` |
| A dropped open leaves nothing | `harness_open::a_harness_open_that_is_dropped_leaves_no_pod_and_no_instance` |
| The seed reaches the box | `harness_open::a_seed_given_to_the_harness_open_reaches_the_box` |
| Web access goes to the port unchanged; mode `none` | `harness_open::web_access_that_reaches_nothing_opens_and_web_access_that_reaches_a_site_is_refused_in_mode_none` |
| A secret in a seed, a write and a command stays out of the records | `harness_open::a_canary_in_a_seed_file_a_write_and_a_command_stays_out_of_the_pod_metadata_and_the_save_record` |
| Each start kind comes back from the adapter (the harness itself is not between the two, except for the first and the next turn of `harness_suite`) | `start_results`, and the first and next turn checks of `harness_suite` |

Tests of the harness crates that use no sandbox, or a harness with no sandbox port, are not changed by the swap and are not run again here.

## What is proved through the public surface only

`KubeCluster` is not exported by `br-runner-sandbox` 0.1.0. Rows that name its methods (`exec`, `watch_pods`, `patch_pod`, `create_pod`, `replica_set`) are proved through effects that the public `Sandbox` shows on the cluster: pods, labels, annotations, store objects and the text of the `PortError`. The order of the single watch events and the exact `NoStatus` variant are only seen through that text.

## Not covered

| Item | Reason |
|---|---|
| Exit code of the keeper process for a save age of 0 or -1 | The keeper binary is not part of this project. The chart and the adapter configuration refuse the value; the tests check both. |
| `shard_max` | Not in 0.1.0. |
| Record lifetime in days (`save.tombstone_ttl_days`) | The unit is days; no timed test is possible. |
| Closed-cluster full flow | Out of this project. |

## Why

| Thing | Why it is the way it is |
|---|---|
| Fault injection with a local TCP gate and a temporary kubeconfig context | The API and the store are cut or slowed without stopping them for other users of the cluster. |
| A child process (`src/bin/save_child.rs`) killed with `SIGKILL` | A crash inside a save cannot be made from inside the test process. |
| Warm pool emptied and keeper stopped in some tests | The keeper deletes pods and old saves on its own clock; the tests need the state to stay as the test made it. |
| Idle keep size set to the maximum in most tests | A release then always deletes the pod, so the next open is a store restore. |
| Namespace and pool names made from the variables | No name of a cluster, store or chart is written in the tests. |
| The restore of `PoolGuard` replaces the whole `spec` with the one it read | A merge patch cannot remove a key of a nested object (the size) that the test added; the replace gives back exactly what was there. |
| No comments in the code | The test names and the check names carry the intent. |
