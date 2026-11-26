# Checks that go through the Rust adapter

These checks need the `br-runner-sandbox` API (`open`, `run`, `read`, `write`, `list`, `save`, `release`, the port 0.2.0 types). They are Rust integration tests to write when the adapter API exists. No Rust code is written yet. Each check is described as Given / When / Then, with the data to prepare, the numbers to record, and the pass limit. The limits are not changed after a measurement.

Common rules for all tests below:
- Test cluster `sbx-build-test`, chart installed, store reached through the in-cluster Service (see the README of the set-up). The test starts from a warm pool of the default pool of client `ca`, mode `none`, unless the check says otherwise.
- Timed tests run one at a time under the timing lock of `run-all.sh` (`names-chart.env`: `TIMING_LOCK`). Each recorded number is written with `record NAME VALUE UNIT`; the load average is added by `record`. A test that runs with a load above 8 is repeated.
- The start time runs from the call of `open` to the moment when the first `run` of `true` returns exit code 0.
- `save`, `restore`, `read`, `write` and `list` execs run as the non-root agent user, not as root. The extract uses `bsdtar --same-owner --numeric-owner -p`. A non-root user cannot `chown` to another user id, so the files of a restored tree have the user id and group id of the agent. Every test below creates its files as the agent user; where a check names user ids (tree equal after restore), the tree uses the agent ids only. Write this limit in the evidence.
- Seeds, states and trees are made from a fixed seed of a pseudo-random generator, so a failed run can be repeated with the same bytes.
- Names of the store keys, of the labels and of the chart values come from the README of the chart and `names-chart.env`.

## Fresh start and restored start (with seed copy-in)

Check text: at least 95 of 100 starts within 500 ms, clock from open request to first accepted `true`, slowest reported; two cases counted separately: fresh start, and start with a restored save. On the Mac the count is smaller than 100 and is stated.

Data to prepare:
- Seed set S: 50 files, 10 MB in total (49 files of 200 KB and one of about 200 KB less, generated from the fixed seed), with the directory depth 1 to 3.
- Stress seed: 500 files, 50 MB (stress run).
- States of 10, 30, 50 and 100 MB: a turn that writes one random file of the size plus 50 files of 1 KB under `/work`, then `save`. Save each state once, keep its turn id.

Given: warm floor at least 3 for the pool; `idle_ttl` as the chart default; seed S in the request.
When (fresh): 20 times in a row, `open` with `parent = None` and seed S, `run true`, `release` (no save), wait until the warm count is back to its floor.
Then (fresh): record median, p95 and slowest in ms. Pass: at least 19 of 20 within 500 ms. An earlier prototype gave 155 ms median, 196 ms slowest.
When (stress seed): the same 20 times with the stress seed. Record the numbers. No pass limit (the check says "limit reported, not 500 ms").
When (restored, per state size 10, 30, 50, 100 MB): 20 times `open` with `parent = state turn` and seed S, `run true`, `release`.
Then (restored): record median, p95, slowest and the count within 500 ms, one set per size, and the split by step (claim, restore stream, seed copy-in, first `true`). Pass limit: the same 19 of 20, reported per size, pass or not. The 100 MB case did not pass in the earlier prototype (733 to 921 ms); report it as measured.
When (read-ahead of the load): repeat the 30 MB case with 4 opens at once. Record only.

## Save keys

Check text: saves that differ in thread only, agent only, turn only are distinct objects; the same key twice gives a defined result.

Given: client `ca`; threads th1, th2; agents a1, a2; turns k1, k2 (UUIDv7).
When: save (th1, a1, k1), (th2, a1, k1), (th1, a2, k1), (th1, a1, k2).
Then: the store holds four manifests under four different keys; each restore returns only its own content (each save has a marker file with its own key).
When: save (th1, a1, k1) a second time with other content.
Then: `Err(SaveFailed{AlreadySaved})`; the first manifest and its parts are unchanged (hash of the manifest and of the parts equal to before); the harness treats it as a no-op. Pass: the second result is the one the spec names (reject).

## No save on a failed turn

Check text: three cases; the next turn starts from the last successful save; no partial save is visible.

Given: a thread with turn k1 saved (tree H1).
Case 1, failed turn: run turn k2 that writes a file and ends with `RunEnd::Failed`. Then: no manifest and no record `done` for k2; the next `open` with `parent = k1` returns `Restored{k1}` and the tree hash is H1.
Case 2, sandbox killed: during turn k2, delete the pod with `--grace-period=0 --force` (and in a second run `kill -9 1` through exec if the pod allows it). Then: `run` returns an error (`ExecStreamLost` or the sandbox-lost error), never an exit code; the harness releases without a save; the next `open` with `parent = k1` gives `Restored{k1}` and H1.
Case 3, runner crash between model end and save end: a test binary runs `save` with a fault point; the test kills the process with `kill -9` (a) after the parts are uploaded and before the manifest is written, (b) after the manifest and before the pod annotation. Use a state of 60 MB with `shard_max = 2` so that a half-written save has one part of two. Then (a): no manifest, the listing of saves shows only complete saves, the orphan parts are deleted by the keeper sweep (record the time); the next `open` gives `Restored{k1}`. Then (b): the manifest exists and the next `open` restores from it (record which turn is restored; the spec says safe).
Pass: all three cases give the last successful save and H1 (case 3b: the turn of the manifest) and no partial save is listed. Record the sweep time for the orphans.

## Save expiry

Check text: each save is a whole independent image; test with x = 60 s: the store deletes the object, no runner call; restore of save 3 after save 1 is deleted gives the full file system; x of 0 or less is rejected.

Given: chart value for x set to 60 (one value, rendered to the keeper and the adapter); the test process stops after the saves (no adapter call after that).
When: save turns k1, k2, k3 in sequence; k1 writes `/work/only-in-1`, k2 writes `/work/only-in-2`, k3 writes `/work/only-in-3`; tree hashes H1, H2, H3. Wait. Poll the store every 5 s.
Then: the manifest and the parts of k1 are gone within `x + skew_margin + sweep_interval` (default 60 + 60 + 60 s); record the real delay; a tombstone exists for k1. A restore of k3 after the deletion of k1 gives hash H3 and all three marker files (so k3 is a whole image, not a delta). Record the object sizes of k1, k2, k3 (a delta would be smaller).
When: stop the keeper (scale to 0) and wait past x for k2. Then: `open` with `parent = k2` returns `Fresh(SaveExpired{..})` (read-side expiry); the store may still hold the object.
When: render the chart with x = 0 and x = -1; start the keeper and the adapter configuration with the same values. Then: `helm template` fails with the save-age error; the keeper process exits with a non-zero code at config load; the adapter constructor returns its config error. Pass: all three refuse.

## Tree equal after restore

Check text: tree of 1 000 files and one 50 MB file: content hash, mode, numeric uid/gid, symlinks, empty directories, mtime to 1 s, executable bit equal.

Given: a tree made as the agent user under `/work`: 1 000 files of 0 B to 50 KB with modes 0644, 0600, 0755, 0750 and 0444; one random file of 50 MB; 20 symlinks (relative, absolute, dangling); 20 empty directories; 10 hard links; distinct mtimes set with `touch -d` (second resolution); files in `/home/agent` too (a save path). Files in `/tmp` and one background process are made on purpose.
When: record the manifest of the tree (`find -printf` with path, mode, numeric uid, numeric gid, size, mtime in seconds, link target; `sha256sum` of every file), `save`, `release`, `open` with the parent, record the manifest again.
Then: the two manifests are equal; mtime equal to 1 s; executable bits equal; `/tmp` content and the background process are not restored (the spec lists them as not kept). Record the save time and the restore time for the tree. Pass: zero differences. User ids and group ids are the ones of the agent user (see the common rules).

## Go back with `parent`

Check text: three turns, go back to turn 1: tree hash equals turn 1; then a new turn starts from turn 1; per agent; the turn id is an id, not a position.

Given: thread th, agents a1 and a2; each agent runs turns k1, k2, k3 (UUIDv7, not in order of value for one run: use ids made in the order k2, k1, k3 to prove that the id is not a position); each turn changes the tree; hashes H1, H2, H3 per agent.
When: `open` for a1 with `parent = k1`.
Then: result `Restored{k1}`, tree hash H1 of a1; the files of k2 and k3 are absent.
When: run turn k4 with `parent = k1` (writes a new file), save; `open` with `parent = k4`.
Then: the tree is H1 plus the file of k4, and no file of k2 or k3. The saves of k2 and k3 still exist until they expire.
Then (per agent): the same steps for a2 give the hashes of a2; a1 and a2 never see each other's files. Pass: all hashes as named.

## A missing save never stops a run

Check text: delete the save, start the turn: a trivial command works within the 500 ms start limit; seed files present; also with a delete during the restore (race).

Given: a saved turn k1 with a 30 MB state and seed S.
When: delete the manifest and the parts of k1 in the store (S3 delete), then `open` with `parent = k1` and seed S; `run true`.
Then: `Ok(Opened{start: Fresh(NoSaveEver or SaveExpired)})`, never `Err`; `true` returns 0 within 500 ms of the `open` (record, 20 repetitions, 19 within 500 ms); the seed files are present (hash of the seed tree equal to S).
When (race): `open` with `parent = k1` and a 100 MB save; a second task deletes the manifest at a random offset between 0 and the restore time (20 repetitions, offsets spread over the whole restore: before the first read, between the parts, after the last part).
Then: `Ok(Opened{start: Fresh(SaveExpired..)})` or `Restored{k1}` (if the delete came after the second manifest read); never `Err`; the box holds no file of a half-restored save (hash of `/work` equals the seed tree when the start is `Fresh`); a half-restored pod is deleted. Record the count of each result. Pass: 20 of 20 without `Err` and without a half-restored tree.

## Named start results

Check text: `NoSaveEver`, `SaveExpired`, `SaveFailed` are different values, each returned by a scripted scenario. The spec says: save failed at end of turn against restore failed, rollback or thread delete, and the lifetime of the record that tells expired from never made.

Scenarios (each one is one test; the result is compared with the enum value, not with a text):
1. `NoSaveEver`: `open` with `parent = None`; and with a parent that has no object at all. The store is unreachable in the first run: the result is still `NoSaveEver` (no store read).
2. `SaveExpired{expired_at: Some}`: save, let the keeper expire it (x = 60), `open` with the parent. A second run: the manifest is present but `made_at + x` has passed and the keeper is stopped: `SaveExpired`.
3. `SaveExpired{expired_at: None}`: remove the tombstone and the manifest, keep the record `done`: the result has no time.
4. `SaveFailed{turn}`: make the manifest upload fail (the store refuses the write: a policy on the key prefix, or a fault proxy in front of the store); `save` returns `Err(SaveFailed{Store})`; the record is `failed`; the next `open` with that parent gives `Fresh(SaveFailed{turn})`. A second run: the record is `started` with no manifest (runner killed): the same result.
5. `RestoreFailed{turn}`: overwrite one part with random bytes; `open` gives `Fresh(RestoreFailed{turn})` and the pod is deleted. Also with one part deleted.
6. `StoreUnavailable`: stop the store container; `open` with a parent gives `Ok(Fresh(StoreUnavailable))`, not `Err`; the tests of the missing-save check still pass after the store is back.
7. Rollback: `parent = k1` gives `Restored{k1}` (see "Go back with `parent`"). Thread delete: `erase_thread(client, thread)` then `open` with the parent gives `Fresh(NoSaveEver)`; the prefixes `saves/`, `records/` and `tombstones/` of the thread are empty.
8. Record lifetime: with the keeper configured for a short tombstone lifetime (a value in seconds in the test configuration; if the chart has days only, state "not tested" and name the value), a save that expired earlier than the lifetime gives `SaveExpired`, and after the lifetime gives `NoSaveEver`.
Then: the five results `NoSaveEver`, `SaveExpired`, `SaveFailed`, `RestoreFailed`, `StoreUnavailable` are pairwise different values. Pass: each scenario returns its own value.

## The images

### Two images with a value change only

Given: chart installed with the value `image` = the test image `br-sandbox-python`; one test program (the Rust test) that opens, writes `/work/f`, runs a version command given by an environment variable, saves, restores, and reads `/work/f`.
When: run the program with `python3 --version`. Then `helm upgrade --reuse-values --set image=<second image>` where the second image is `debian:bookworm-slim` plus R (`R --version`), wait for the new warm pods, run the same program with `R --version`.
Then: both runs pass; the diff of the repo and of the program between the two runs is empty (zero code lines); only the `image` value of the values file changed (print `helm get values` before and after). The second image has `/bin/sh`, `cat`, `find` only (the image contract). Record the time for the new pool to be warm.

### New push under the same tag

Given: a local registry that the node can pull from (the set-up does not make one: the test set-up adds it, or the test starts the registry container on the Docker network of the cluster); image `reg/test:moving` v1 with the file `/etc/image-version` = 1; chart value `image` = that tag; `warm_max_age_seconds` set to 120 for the test (`names-chart.env`: `WARM_MAX_AGE_VALUE_KEY`); `imagePullPolicy: Always`.
When: confirm `read /etc/image-version` is 1 from a new box; push v2 (same tag, content 2); note the push time. Every 5 s open a box, read the file, release.
Then: the first box that reads 2 comes within `warm_max_age_seconds` of the push time. Pass: delay at most the value (record the delay and the value). Also check that a warm pod is used one time only (a claimed pod never goes back to the pool) and that the image id of the new warm pods differs from the old one.

### `ImageUnavailable`

Given: pool floor 1, cold create on, `open_timeout` = 5 s for the test.
When: fill the quota of the namespace with filler pods (or cordon the node) so that no new pod becomes Ready; call `open`.
Then: `Err(ImageUnavailable)` within `open_timeout` plus 1 s; the keeper has not written the failing-pool condition on the ReplicaSet. When the filler pods are removed, the same `open` call succeeds on retry (the error is retryable).

### `ImageFailing`

Given: the pool with an image that cannot start.
When: `helm upgrade --set image=registry.invalid/none:1` (pull failure); wait for the keeper condition on the ReplicaSet (the condition type is named in the keeper README). Call `open`. Second run: an image that starts and crashes at once (command `false`).
Then: `Err(ImageFailing)` (not `ImageUnavailable`) in both runs; the condition shows on the ReplicaSet. Record the time from the upgrade to the condition and to the error. After `helm upgrade` back to the good image: the condition clears and `open` works. Pass: each error comes from its own scenario.

## Harness test suite with the adapter

Given: the harness repo `br-llm-graph-rs` at the release that uses contract 0.2.0 (`br-agentic-harness` 0.2.0), the adapter `br-runner-sandbox` 0.1.0, the test cluster.
When: run the test suite of the harness with the adapter in place of `MemorySandbox` (a test crate in `br-jobs-runner-kit` that builds the harness cases against the adapter through the port; the suite is not edited).
Then: all cases pass unchanged. A case that cannot run against a real sandbox (it needs the double only) is listed by name with the reason. Any port change that the suite needs is recorded as a cost line (change, contract minor, harness minor, consumers to repin) and is not made silently. Pass: no case fails; the list of excluded cases is empty or accepted in review.

## Full flow through the adapter (second pass)

The script `s12-closed-cluster.sh` proves the flow on the closed cluster with `kubectl` and `bsdtar`. When the adapter exists, run the same flow once through the adapter on `sbx-build-closed`: `open`, `run`, `save`, `release`, `open` with the parent (restore), a call in mode `allow` through the proxy, a scale of the pool. Pass: every step passes with no route to the internet; the proxy logs hold no vendor host.
