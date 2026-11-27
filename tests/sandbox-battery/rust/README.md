# Sandbox battery: Rust tests

Tests that run the released `br-runner-sandbox` adapter (tag `br-runner-sandbox/v0.1.3`) against the live test cluster, the real object store and the real tools image. This is a separate cargo project (own `[workspace]`), not a member of the kit workspace. It uses the released harness crates at 0.2.0 from `br-llm-graph-rs`.

No test is ignored. A test that cannot reach the cluster, the store or a named image fails with a message that names the missing thing.

## Run

```sh
cd tests/sandbox-battery/rust
set -a
. ../names.env
. ../scenarios/names-chart.env
. <creds.env of the set-up>
set +a
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Tests that change the cluster (scale the pool, stop the keeper, cordon the node, change helm values) restore it when they end, also on failure. A restore that fails is written to `checks.tsv` as a failed check and fails the test. Timed tests take the lock `TIMING_LOCK` (a `mkdir` lock, the same one as the shell scripts), so they never overlap with a shell scenario. Run one at a time: `--test-threads=1`. The lock folder holds a file `pid` with the process id of the holder. When the holder is gone (`kill -0` fails with "No such process"), the next test takes the lock over; a live process, also one of another user ("Operation not permitted"), is never taken over. Limit: a lock folder with no `pid` file (a holder that stopped between the folder and the file, or a lock made by a shell scenario) is never taken over and must be removed by hand; a reused pid of a dead holder also counts as alive. The wait for the lock ends with an error after six hours. Unit tests of the take-over: `cargo test --lib lock`.

Results go to `$EVIDENCE/checks.tsv` (name, pass or fail, detail, time) and `$EVIDENCE/measurements.tsv` (name, value, unit, load 1 min, load 5 min, time), in the format of `../lib.sh`. A test ends with an error when one of its checks failed, when it recorded no check, or when an evidence file cannot be written, so the exit code of `cargo test` is the verdict.

No CI job of this repository compiles or runs this project: it is outside the kit workspace and its tests need a live cluster, a store and the images. The checks above (`cargo fmt --check`, `cargo clippy`, `cargo test`) are run by hand.

## Variables

All names are read in `src/env.rs` (one `Env` struct). A missing variable fails the test with the variable name and the file it comes from.

| Variable | Comes from |
|---|---|
| `CTX_TEST`, `NODE_TEST`, `NS_PREFIX`, `CLIENT_A`, `NET_NONE`, `RS_PREFIX` | `scenarios/names-chart.env` |
| `SANDBOX_CONTAINER`, `SEED_CONTAINER`, `SEED_PATH`, `AGENT_BSDTAR`, `SEED_BSDTAR`, `SANDBOX_PYTHON` | `scenarios/names-chart.env` |
| `HELM_RELEASE`, `HELM_NAMESPACE`, `SYSTEM_NS`, `KEEPER_DEPLOY`, `RUNNER_NS`, `RUNNER_SA` | `scenarios/names-chart.env` |
| `VALUES_IMAGE_KEY`, `WARM_MAX_AGE_VALUE_KEY` | `scenarios/names-chart.env` |
| `S3_ENDPOINT_HOST`, `S3_BUCKET`, `S3_REGION`, `S3_PATH_STYLE`, `S3_ACCESS_KEY`, `S3_SECRET_KEY` | `creds.env` |
| `WORK` or `BATTERY_WORK`, `EVIDENCE`, `TIMING_LOCK`, `CHART_DIR`, `CHART_VALUES_FILE` | optional; defaults are the work folder `~/sbx-battery-work/battery`, `$WORK/evidence`, `$WORK/timing.lock.d`, the chart of this repository, `$WORK/values-test.yaml` |

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
| Second image by value, same tag reaches a new warm pod within `warm_max_age_seconds` (no box is opened while waiting, so the pool is not renewed by claims), two image errors | `images`, `open_images` |
| Harness suite with the adapter | `harness_suite`, `harness_open`, `harness_excluded` (see below) |
| Hard links, executable bit, image file deleted by the agent | `properties` |
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
| No comments in the code | The test names and the check names carry the intent. |
