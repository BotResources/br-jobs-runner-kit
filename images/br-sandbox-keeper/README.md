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

The tag of a published image is the version of the crate (for example `0.1.0`). `.github/workflows/keeper-image.yml` pushes it when `crates/br-sandbox-keeper/Cargo.toml` changes its version on `main`, and never replaces a tag that exists. A pull request that changes the crate builds and tests the image and pushes nothing. `images.yml` also builds every folder of `images/`, with the tags `sha-<7>` and `pr-<number>-<7>`.

## Tests

`test.sh` runs for each platform: the architecture of the image, the numeric non-root user, the entrypoint, the root certificates, the refusal to start with no configuration and with a configuration that has an unknown key.

```sh
docker build -t br-sandbox-keeper:local -f images/br-sandbox-keeper/Dockerfile .
images/br-sandbox-keeper/test.sh br-sandbox-keeper:local linux/arm64
```
