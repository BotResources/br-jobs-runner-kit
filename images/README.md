# Container images

Every folder `images/<name>/` that holds a `Dockerfile` is an image. The repository root is the build context, so a `Dockerfile` can copy `LICENSE`, `crates/` and the like. `<name>` matches `^[a-z0-9][a-z0-9._-]*$` (a registry name); a folder with another name fails the plan.

Images: [`br-sandbox-tools`](br-sandbox-tools/README.md), [`br-sandbox-python`](br-sandbox-python/README.md), [`br-sandbox-keeper`](br-sandbox-keeper/README.md), [`br-sandbox-check`](br-sandbox-check/README.md).

## Workflows

| File | Trigger | Job |
|---|---|---|
| `.github/workflows/images.yml` | pull request, push to `main`, manual dispatch | Decides `push` and `tag` from the event with `.github/scripts/images-plan.sh`, then calls `images-build.yml` for `images/`. On pull requests it calls it a second time with `push` false for the fixture folder (see below). Its first job is named `plan images`. |
| `.github/workflows/images-build.yml` | `workflow_call` only | Builds, tests and, when asked, pushes. It reads no event of its caller: everything it needs is an input. |

`images.yml` runs only when a path of its filter changes: `images/**`, `.github/fixtures/images/**`, `.dockerignore`, the two workflow files, `.github/scripts/images-*.sh`, `.github/scripts/test-images-*.sh` and `.github/scripts/test-tools-image*.sh`. An image that builds from `crates/` or from another folder adds those paths to the `paths` lists of `images.yml` (both `pull_request` and `push`).

### Push and tag policy of `images.yml`

| Event | Push | Tag |
|---|---|---|
| pull request from this repository | yes | `pr-<number>-<first 7 characters of the head commit>` |
| pull request from a fork, by Dependabot (read-only token), or whose head repository is empty or unknown | no | the same tag, used only as the `version` label |
| push to `main` | yes | `sha-<first 7 characters of the commit>` |
| push to another branch | no | `sha-<7>` |
| manual dispatch | only with the input `push` set to true | `sha-<7>` |

A pull request of this repository pushes to the organisation registry before any review, except for an image that holds a marker file (next section). A tag is not immutable: a new run of the same commit moves it. A deployment pins the digest, never the tag. The tags `pr-<number>-<7>` are never removed by the workflow: each pull request adds them to the package and they stay until someone deletes them in the registry; so do the untagged platform digests of a failed run.

### An image that is pushed by a push to `main` only

An image folder that holds the empty file `PUSH_ON_MAIN_ONLY` is built and tested on both platforms by every event, but it is pushed to the registry only by a push to `main`. Use it for an image that is published by a push to `main` only. For such an image a pull request, a manual dispatch with `push` true and every other event take the no-push path: each platform is built with `--load` and tested in the Docker of the job, with no registry login and no registry write (the build cache of the workflow is the GitHub Actions cache, not the registry). The other images keep the policy of the table above. When the push is from a push to `main`, the image is pushed like any other.

The marker is read by `.github/scripts/images-select-push.sh`: the job `plan` of `images-build.yml` gives it the discovered images, `push` and the input `hold_marked`, and it prints the list of images to push. `images.yml` sets `hold_marked` to false only for a push to `main` (`hold_marked` is an output of `images-plan.sh`) and gives true for any other value, a missing one included; the input of `images-build.yml` defaults to true, so a caller that says nothing holds marked images back. `images-check-gates.sh` pins this wiring (the plan outputs of both workflows, the `PUSHED` decision of the step `names`, and the `PUSH`, `HOLD_MARKED` and `IMAGES` of the step that selects the pushed images), and `test-images-gates.sh` refuses a copy that changes any of them. The job `build` takes the push decision for each image from that list and never from the input `push`; `manifest` makes lists for the pushed images only. The marker is read from the head of the pull request: a pull request of this repository that deletes the marker removes the hold for that pull request, so the marker guards against a mistake and is not an access control. To stop holding an image back, delete its marker file. Only `br-sandbox-check` holds the marker today.

### Proof of the no-push path

The path that a fork, a Dependabot pull request and a default dispatch take (`push` false: load the image, test the local reference, no login, no artifact, no manifest list) runs on every pull request of this repository, on the fixture folder `.github/fixtures/images/` (the image `no-tests`: `FROM scratch`, one file, no `test.sh`). The job `fixture images` of `images.yml` calls `images-build.yml` with `push: false` and `images_dir: .github/fixtures/images`; the job `fixture output` fails unless the output `digests` is `{}`. The fixture also proves the path of an image with no `test.sh`. The fixture is never pushed (the plan refuses `push` true with any `images_dir` other than `images`) and is not an image of the product: `images/` is the only folder that is listed as images. A same-repository pull request takes the push path for `images/`, so a second no-push build of `images/` would only repeat the push build.

### Inputs and output of `images-build.yml`

| Input | Type | Meaning |
|---|---|---|
| `image` | string, default empty | Name of one folder of `images_dir`; empty builds every image. |
| `images_dir` | string, default `images` | Folder that holds the image folders, relative to the root of the repository: plain folder names separated by `/`, no `.` or `..` component, no leading `/`; the plan refuses anything else. With `push` true it must be `images`. |
| `push` | boolean, required | `true` pushes the tested images; `false` builds and tests only. |
| `hold_marked` | boolean, default true | `true`: an image whose folder holds `PUSH_ON_MAIN_ONLY` is built and tested but not pushed, whatever `push` says. `images.yml` gives false for a push to `main` only. |
| `tag` | string | Tag of the manifest list. Required when `push` is true (letters, digits, `_`, `.`, `-`; up to 128 characters; the plan refuses anything else). With `push` false it is only used in the `version` label. |
| `ref` | string, default empty | Commit to check out, build and name in the `revision` label. Empty builds the commit of the caller. `images.yml` passes the head commit of a pull request, so the label and the tree agree (the pull request is built as its head, not as the merge with `main`). |

| Output | Meaning |
|---|---|
| `digests` | JSON object, image name to `ghcr.io/<owner>/<image>@sha256:<digest of the manifest list>`. `{}` when nothing was pushed. |

A workflow that calls `images-build.yml` grants `contents: read` and `packages: write` to the job that calls it (the called jobs need `packages: write` to push), passes `push` and `tag` itself, and calls it once for each workflow run (the names of the artifacts it uploads hold the image name and the architecture, not the call).

```yaml
jobs:
  images:
    permissions:
      contents: read
      packages: write
    uses: ./.github/workflows/images-build.yml
    with:
      push: true
      tag: v1.2.3
```

### What is built, tested and pushed

1. Both platforms, `linux/amd64` and `linux/arm64`, are always built, each in its own job (the foreign one under QEMU). An image cannot opt out of a platform.
2. `push` false, or an image held back by its marker: each job builds with `--load` and tests the loaded image. Nothing leaves the runner.
3. `push` true and the image not held back: each job builds and pushes the platform image by digest only, with no tag (`push-by-digest`), pulls that digest from the registry, and runs the label check and the tests on the pulled image. The pushed bytes are the tested bytes. The digest of each platform is kept as an artifact.
4. A job `manifest list` runs for the images that were pushed, and only when every build job of the run passed (`needs: [plan, build]` with no `always()`, `cancelled()` or `failure()` in its condition; `.github/scripts/images-check-gates.sh` checks it in the plan job of `images.yml`, and `test-images-gates.sh` proves that the check refuses a workflow that breaks it). It creates the manifest list with `docker buildx imagetools create`, from the two digests and from nothing else, under `ghcr.io/<owner>/<image>:<tag>`, checks that the list holds `linux/amd64` and `linux/arm64`, and reports the digest of the list.

If a test fails, the two untagged platform digests stay in the registry (a registry has no atomic push of two images), and the tag and the list do not exist. A failed build of one image blocks the lists of all images of the run.

No provenance and no SBOM attestation is added: they add `unknown/unknown` entries to the list, which then holds more than the two platforms.

### What an image must have

- A label `org.opencontainers.image.licenses` in its `Dockerfile`. The workflow sets `source`, `version` and `revision`; `.github/scripts/images-check-labels.sh` fails the build when one of the four is empty.
- Optionally `images/<name>/test.sh`. When the file exists, the workflow runs `bash images/<name>/test.sh <image reference> <platform>` after the label check, on each platform; a non-zero exit fails the job. The reference is the loaded image, or `ghcr.io/<owner>/<name>@sha256:<digest>` when pushing. The environment variable `RESULTS_FILE` names a file; the script appends one line for each test, and the workflow copies the file into the job summary. An image with no `test.sh` is built, label-checked and pushed without a test.

### Tests of the workflows' scripts

| Script | Proves |
|---|---|
| `.github/scripts/test-images-plan.sh` | The push and tag policy for each event of `images.yml`; the refusal of other events; the folder and image name rule; `images-build.yml` has the trigger `workflow_call` only, never reads `github.event_name` or the event of its caller, and has no `concurrency` block; `images.yml` calls it, and names `images.yml` in its own concurrency group. |
| `.github/scripts/test-images-plan.sh` (more) | A pull request with an empty or unknown head repository does not push; the fixture folder holds the image `no-tests` with no `test.sh` and is not listed in `images/`; `images.yml` calls the fixture job on pull requests only, with `push: false`, and lists the fixture paths in both path filters. |
| `.github/scripts/test-images-plan.sh` (marker) | `hold_marked` is false for a push to `main` only. The selection of the images to push, with a marked image and an unmarked one: a pull request of this repository pushes the unmarked one only; a push to `main` pushes both; a fork, Dependabot, a push to another branch and a dispatch without `push` push none; a dispatch with `push` holds the marked one back; a folder named like the marker is no marker; a value that is not true or false, or a list of images that is not a list of registry names, is refused. In `images/`, only `br-sandbox-check` holds the marker, a pull request selects the other three images, and a push to `main` selects all four. `images-build.yml` never reads the input `push` in its job `build`, makes the manifest lists from the pushed images, and defaults `hold_marked` to true. |
| `.github/scripts/test-images-gates.sh` | `images-check-gates.sh` accepts `images-build.yml` and refuses a copy where `manifest` does not need `build`, where `manifest` or `digests` has `always()`, `cancelled()` or `failure()` in its condition, where a job sets `continue-on-error`, or where a job is missing, where the job `build` reads the input `push`, where the login step of `build` runs for a reason other than the per-image push decision, and where the wiring of the hold is changed (the `PUSHED` decision of the step `names`; the `PUSH`, `HOLD_MARKED` and `IMAGES` of the step `pushed`; the plan output `hold_marked` of `images.yml` and its fail-closed call of `images-build.yml`). Its first argument is `images-build.yml`, its second `images.yml`. The check of the expected test ids of `br-sandbox-tools` names a missing result, a result of an unlisted test, an empty or absent result file; the exit gate turns an early `exit 0` into a failure. |
| `.github/scripts/test-images-scripts.sh` | The helper scripts refuse what they must: a missing label, a malformed digest, a missing or equal digest pair, a list with the wrong platforms, a tag that is not valid, an `images_dir` that is absolute, empty, holds `.` or `..` or a character outside plain names, or is not `images` when pushing. It uses a stand-in for `docker` where a registry would be needed. |
