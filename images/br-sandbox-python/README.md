# br-sandbox-python

Test and reference image of the sandbox. Each project defines its own image; this one shows the contract and serves the battery.

## Content

| Part | Version |
|---|---|
| Base | `python:3.12-slim` (Debian, multi-arch amd64 + arm64), pinned by index digest |
| LibreOffice | `libreoffice-core`, `-writer`, `-calc`, `-impress` from Debian, plus `fonts-dejavu-core` |
| Python libraries | pandas 3.1.1, openpyxl 3.1.5, python-docx 1.2.0, python-pptx 1.0.2, setuptools 84.2.1 |
| Other | `procps` (`ps`) |

`requirements.txt` pins the five libraries and all their transitive dependencies.

## Contract

The image satisfies the sandbox image contract: `/bin/sh`, `cat` and `find`, for the architecture of the nodes. It holds no agent code and has no `USER` instruction. It works as any numeric non-root uid with no passwd entry.

## Build and test

```sh
docker build -t br-sandbox-python:local images/br-sandbox-python
images/br-sandbox-python/smoke.sh br-sandbox-python:local
```

`smoke.sh` runs the image as uid 12345 with all capabilities dropped, and converts a docx, an xlsx and a pptx to pdf with headless LibreOffice.

## Use

Give the chart the image by digest:

```yaml
image: ghcr.io/botresources/br-sandbox-python@sha256:<digest>
```

A tag is accepted; a digest is recommended for production.

With a read-only root filesystem, LibreOffice needs a writable `/tmp` (an `emptyDir` or tmpfs).

## Why

| Thing | Why |
|---|---|
| Base pinned by index digest | The same bytes on every build and on both architectures |
| `/work` and `/home/agent` mode 1777 | The chart sets a numeric `runAsUser`; any uid must write there |
| `ENV HOME=/home/agent` | The uid has no passwd entry; LibreOffice and Python need a writable `$HOME` for a fresh profile |
| LibreOffice profile set to `/home/agent/.libreoffice` in `bootstraprc` | With no passwd entry the default profile path is `$HOME/.config/libreoffice/4`, and LibreOffice does not create `$HOME/.config`: it exits with "User installation could not be completed". The new path needs only `$HOME` to exist, also when `/home/agent` is an empty volume |
| `--no-install-recommends` | Keeps the image small; no X11, no Java |
| `procps` | Process-listing test steps call `ps` |
| `setuptools` | The local-wheel install test step needs it; python 3.12 slim does not ship it |
| No `bsdtar`, no agent code | The sandbox brings `bsdtar` from `br-sandbox-tools`; the image does not depend on any tar |
