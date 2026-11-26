# br-sandbox-python

Test and reference image of the sandbox. Each project defines its own image; this one shows the contract and is the reference image for tests of the sandbox.

## Content

| Part | Version |
|---|---|
| Base | `python:3.12-slim` (Debian, multi-arch amd64 + arm64), pinned by index digest |
| LibreOffice | `libreoffice-core`, `-writer`, `-calc`, `-impress` from Debian, plus `fonts-dejavu-core`; apt versions float |
| Python libraries | pandas 3.1.1, openpyxl 3.1.5, python-docx 1.2.0, python-pptx 1.0.2, setuptools 84.2.1 |
| Other | `procps` (`ps`); apt version floats |

## What is pinned

The base image is pinned by index digest. The Python packages are pinned by exact version in `requirements.txt`, which lists the five libraries and all their transitive dependencies; `test.sh` checks that the installed packages equal this file and that `pip check` passes. The Python packages have no hashes: versions are pinned, hashes are not.

The Debian packages (LibreOffice, fonts, `procps`) are not pinned: each build takes the versions of the Debian archive at that time, so two builds of the same commit can differ.

## Contract

The image satisfies the sandbox image contract: `/bin/sh`, `cat` and `find`, for the architecture of the nodes. It holds no sandbox tool (`bsdtar` and `pause` are absent) and no agent code, and has no `USER` instruction. It is tested as uid 12345 (gid 12345), which has no passwd entry, and as uid 54321 with gid 0.

## Build and test

```sh
docker build -t br-sandbox-python:local -f images/br-sandbox-python/Dockerfile .
images/br-sandbox-python/test.sh br-sandbox-python:local linux/arm64
```

`test.sh` starts the image with all capabilities dropped, `no-new-privileges` and no network. It checks the contract, the Python packages, and the conversion of a docx, an xlsx and a pptx to pdf with headless LibreOffice. The conversion is tested as uid 12345, as uid 54321 with gid 0, with a read-only root filesystem and tmpfs on `/tmp`, `/work` and `/home/agent`, and with an empty volume on `/home/agent`. A fixed list of check names makes `test.sh` fail when a check did not execute or an unlisted check did.

## What the image needs from its caller

- An image reference by digest (a tag is accepted; a digest is recommended for production).
- A numeric non-root user id. The user id needs no passwd entry.
- Writable `/work` and `/home/agent`. Both are mode 1777 in the image; a volume or tmpfs mounted on them must be writable by that user id.

A read-only root filesystem works when `/tmp`, `/work` and `/home/agent` are tmpfs.

## Why

| Thing | Why |
|---|---|
| Base pinned by index digest | The base is the same on every build and on both architectures |
| `/work` and `/home/agent` mode 1777 | The caller sets a numeric user id; any uid must write there |
| `ENV HOME=/home/agent` | The uid has no passwd entry; LibreOffice and Python need a writable `$HOME` for a fresh profile |
| LibreOffice profile set to `/home/agent/.libreoffice` in `bootstraprc` | With no passwd entry the default profile path is `$HOME/.config/libreoffice/4`, and LibreOffice does not create `$HOME/.config`: it exits with "User installation could not be completed". The new path needs only `$HOME` to exist, also when `/home/agent` is an empty volume |
| `--no-install-recommends` | Keeps the image small; no X11, no Java |
| `procps` | Process-listing test steps call `ps` |
| `setuptools` | The local-wheel install test step needs it; python 3.12 slim does not ship it |
| No `bsdtar`, no `pause`, no agent code | The sandbox brings its own tools; the image holds none |
