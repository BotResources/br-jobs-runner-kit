# br-sandbox-keeper

The image of the binary of the crate [`br-sandbox-keeper`](../../crates/br-sandbox-keeper/README.md): the controller of the sandbox pools.

## Content

| Part | Value |
|---|---|
| Build | `rust:1.98.1-slim-bookworm`, pinned by index digest, on the build platform; the binary is cross-compiled with `--locked` for `aarch64-unknown-linux-musl` or `x86_64-unknown-linux-musl`, static, with the symbols stripped |
| Final stage | `FROM scratch` |
| Files | `/br-sandbox-keeper` (the binary) and `/etc/ssl/certs/ca-certificates.crt` (the system root certificates of the build image) |
| User | `65532:65532` |
| Entrypoint | `/br-sandbox-keeper` |

The image has no shell and no other file. The binary reads its configuration from the file named by `BR_SANDBOX_CONFIG` and the store credential from `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`.

## Tag

The tag of a published image is the version of the crate (for example `0.1.0`). `.github/workflows/keeper-image.yml` pushes it in two cases, and only on `main`: a push to `main` where the version in `crates/br-sandbox-keeper/Cargo.toml` changed, and a manual run (`workflow_dispatch`) with `push=true`. It never replaces a tag that exists: `.github/scripts/keeper-image-published.sh` asks the registry, and only a clear "not found" for the tag lets the push go on; any other answer (network, authorization, rate limit) fails the run. A pull request that changes the crate, or a manual run without `push=true`, builds and tests the image, and `keeper-image.yml` pushes nothing there. The check "the tag is already published" and the push are not atomic: they are steps of two jobs, and the step that makes the tag (`docker buildx imagetools create` in `.github/scripts/images-merge.sh`) does not test again that the tag is absent, so two runs that both read "not found" would both push. The workflow narrows this and does not close it. Its `concurrency` group is `keeper-image-<ref>` and `cancel-in-progress` is true only for a pull request: two runs on `main` do not run at the same time, and a run that waits starts its check after the first run has ended. Nothing stops a push to the same tag from outside this workflow. `images.yml` also builds every folder of `images/`, with the tags `sha-<7>` and `pr-<number>-<7>`.

## Tests

`test.sh` runs for each platform: the architecture of the image, the numeric non-root user, the entrypoint, the root certificates, the refusal to start with no configuration and with a configuration that has an unknown key.

```sh
docker build -t br-sandbox-keeper:local -f images/br-sandbox-keeper/Dockerfile .
images/br-sandbox-keeper/test.sh br-sandbox-keeper:local linux/arm64
```
