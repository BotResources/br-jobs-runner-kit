# br-sandbox-tools

The tools that every sandbox pod needs, in one `FROM scratch` image: a static `bsdtar` (libarchive) and `pause`. The user image of a project needs no `tar` and no agent code; it only needs `/bin/sh`, `cat` and `find`.

Registry path: `ghcr.io/botresources/br-sandbox-tools`, built by `.github/workflows/images.yml`, one manifest list for `linux/amd64` and `linux/arm64`. A deployment pins the digest of the list.

## Content

| Path | What | Mode |
|---|---|---|
| `/sbx/bin/bsdtar` | libarchive 3.8.11, `bsdtar` only, static, musl | 0555 |
| `/sbx/bin/pause` | our own reaper (`pause.c`, 44 lines), static, musl | 0555 |
| `/sbx/tools.tar` | pax tar of `bin/`, `bin/bsdtar`, `bin/pause`, `VERSION`; owner 0:0; `bin/` is 0755 | 0644 |
| `/sbx/VERSION` | `libarchive <ver>; musl <ver>; pause <ver>; rev <git sha>` | 0444 |
| `/sbx/LICENSES/` | `libarchive-COPYING`, `musl-COPYRIGHT`, `br-sandbox-tools-Apache-2.0` | 0444 |

No shell, no `/etc`, no `cp`. Size: about 1 MB per architecture (limit 5 MB). OCI labels: `source`, `version`, `revision`, `licenses`, `title`.

## Contract

Mount path: a chart value, default `/opt/sbx`. Programs: `<mountPath>/bin/bsdtar`, `<mountPath>/bin/pause`.

Init container (copy without a shell), run as the same security context as the sandbox container:

```
/sbx/bin/bsdtar -x -p --no-same-owner -f /sbx/tools.tar -C <mountPath>
```

The save scope is the paths `/work` and `/home/agent`; the tools directory is outside it and has no marker file. The `emptyDir` is `readOnly: true` in the sandbox container. Running the init command a second time on the same volume exits 0.

| Call | Argv |
|---|---|
| save | `bsdtar --create --file - --format pax --numeric-owner --no-xattrs --no-acls --no-fflags --options hdrcharset=BINARY -C <root> -- <path>...` |
| restore and copy-in, as root with the 7 capabilities of the sandbox pod | `bsdtar --extract --file - --preserve-permissions --same-owner --numeric-owner --no-xattrs --no-acls --no-fflags -C <dir>` |
| write as a non-root user | the same with `--no-same-owner` in place of `--same-owner`; files then belong to that user |
| version | `bsdtar --version`, first line `bsdtar 3.8.11 - libarchive 3.8.11 libb2/bundled` |

Rules that the callers (save, restore and seeding code) must keep:

1. **Exit rule.** Exit 0 means every entry is done and nothing was reported. Any other exit fails the step. Warnings (a failed `chown`, a refused xattr, a file that cannot be read) give exit 1 and a message on stderr; the entries that could be written are on disk.
2. **End of input.** `bsdtar -x -f -` does not stop at the end-of-archive blocks: it waits for end of input on stdin. The caller must close stdin after the last byte of the tar. A writer that keeps the pipe open holds the process for as long as it stays open.
3. **Empty input exits 0** and extracts nothing. The caller checks that it sent bytes.
4. **Truncated tar** (cut inside a file) exits 1 with `Truncated tar archive detected while reading data`. A tar that only lacks its end-of-archive blocks exits 0.
5. **File names.** `--options hdrcharset=BINARY` goes on create only. On extract the option makes `bsdtar` exit 1 (`A character-set conversion not fully supported on this platform`) and names are not restored. Without the option on create, non-UTF-8 names (and, in the C locale, any non-ASCII name) give a `Can't translate pathname ... to UTF-8` warning on stderr with exit 0; names still arrive byte for byte.
6. **Owner.** `--same-owner` needs `CHOWN` (root with the 7-capability set). As uid 10001 with no capability it exits 1 for each entry that belongs to another id, and still writes the entry as the caller. A tar whose entries all belong to the caller exits 0. An id that is not mapped in the user namespace gives `Can't set user=..: Invalid argument` and exit 1; the file is written, owned by root of the namespace.
7. **Extended attributes.** Without `--no-xattrs`, a `security.*` entry in the tar gives `Cannot restore extended attributes` and exit 1. The flags `--no-xattrs --no-acls --no-fflags` are part of every call above.
8. **Read side.** `create` as a non-root user on a directory of root: files that are readable are stored; a `0600` file or a `0700` directory of root gives `Can't open ...: Permission denied` and exit 1.
9. **Second container.** The seed is written by `pods/exec` into a container that runs `/sbx/bin/pause`, as root of the user namespace, with `readOnlyRootFilesystem: true`. `docker exec -i /sbx/bin/bsdtar -x -f - ... -C <shared volume>` works; `bsdtar` writes only to the volume.

`pause`: pid 1 sleeps, reaps every child on SIGCHLD (`waitpid(-1, WNOHANG)` in a loop, and once at start), and exits 0 at once on SIGTERM or SIGINT. It reads no argument and no input and prints nothing.

## Why

| Thing | Why it is the way it is |
|---|---|
| `FROM scratch`, no shell | The sandbox needs two programs. A shell or busybox adds attack surface and GPL duties. |
| `bsdtar` and not GNU tar | Static build with no copyleft: libarchive is BSD-2-Clause. GNU tar is GPL. |
| musl, `-all-static -no-pie` | glibc static brings LGPL duties. `-all-static` is needed: libtool drops a plain `-static`, which gives a binary with an interpreter that `scratch` does not have. `-no-pie` keeps the program free of a dynamic section, so the test "no `PT_INTERP`, no `PT_DYNAMIC`" is exact. |
| `--without` for every codec and crypto library | The wire is raw tar; zstd runs in the runner. `libb2/bundled` in the version line is the BLAKE2 code that libarchive carries in its own source; it links nothing. |
| Our own `pause.c` | 44 lines, one behaviour, no upstream copy and no NOTICE duty. The reaper loop runs once at start, so a child that died before the handler was set does not stay a zombie. |
| `--no-same-owner` in the init command | The init container extracts files for the user that runs it. The programs are mode 0555, owner root of the namespace. |
| `bin/` is 0755 in the tar | A second extraction by a non-root init user must be able to replace the files in `bin/`. |
| Fixed mtime 1 and a fixed entry list in `tools.tar` | The tar carries no build time. |
| Two digests of the sources | libarchive 3.8.11 is checked against the digest of the GitHub release and of libarchive.org; musl 1.2.6 against the sha512 in the Alpine `APKBUILD` of the same release. `ADD --checksum` fails the build on a change. |
| The base image is pinned by digest | The build must not change when Alpine publishes a new 3.24 build. A new libarchive or musl CVE means a new pin and a new image. |
| No `provenance` and no `sbom` in the workflow | They add `unknown/unknown` entries to the manifest list. The list holds the two platforms only. |
| `hdrcharset=BINARY` on create only | Measured: on extract it fails in this build. |

## Licences

| Product | Version | SPDX | In the image |
|---|---|---|---|
| libarchive | 3.8.11 | BSD-2-Clause (some files public domain, CC0, Apache-2.0 or OpenSSL licence, see `COPYING`) | `bsdtar` statically linked; text in `/sbx/LICENSES/libarchive-COPYING` |
| musl libc | 1.2.6 | MIT | statically linked into both programs; text in `/sbx/LICENSES/musl-COPYRIGHT` |
| `pause.c`, Dockerfile, CI | n/a | Apache-2.0 | `/sbx/LICENSES/br-sandbox-tools-Apache-2.0` |

No copyleft product. libacl (LGPL-2.1) is not built in: `--disable-acl`.

## Build

```
docker buildx build --platform linux/arm64 --build-arg REVISION=$(git rev-parse HEAD) --load \
  -t br-sandbox-tools:test -f images/br-sandbox-tools/Dockerfile .
bash .github/scripts/test-tools-image.sh br-sandbox-tools:test linux/arm64
```

The workflow runs the same script for each architecture, before the push. The tests need Docker, `python3` and a pull of `debian:bookworm-slim` (by digest) as the sandbox-like image.

## Tests and measured results

Run on 2026-11-26 on a Mac (Docker 29.4.3, Docker Desktop VM, 8 CPU). `linux/arm64` ran natively; `linux/amd64` ran under emulation (timings of amd64 are not representative). Each test prints one `RESULT` line. Tests 1 to 10 with their sub-results; 4b and 4d record a behaviour and do not judge it.

| Test | Result on `linux/arm64` (native) and `linux/amd64` (emulated) |
|---|---|
| 1 Static binaries, version | PASS both. No `PT_INTERP` and no `PT_DYNAMIC` in `bsdtar` (854 KB arm64, 809 KB amd64) and `pause` (66 KB, 17 KB). Version line `bsdtar 3.8.11 - libarchive 3.8.11 libb2/bundled`; no zlib, bz2lib, liblzma, libzstd, liblz4, openssl, nettle, mbedtls, xml2, expat or ACL library listed. |
| 1b Size | PASS. 1 026 275 bytes arm64, 983 530 bytes amd64 (limit 5 MB). |
| 2 Init extraction | PASS both. The tools image extracts into a docker volume with the 7-capability set; `debian:bookworm-slim` mounts it read-only and runs both programs by absolute path. Programs mode 0555, owner 0:0, `bin/` 0755; top level holds `VERSION` and `bin` only; a write on the mount fails (`Read-only file system`); a second extraction exits 0 with no output. |
| 3 Tree round trip | PASS both. 1 040 entries, 54.5 MB tar (1 000 files, one 50 MB file, 4 symlinks incl. dangling, empty directories with modes and ids, executable bits, a sub-second mtime): mode, uid, gid, symlink targets, SHA-256 of every file, mtime to the second and the sub-second mtime to the nanosecond are equal after restore (3 086 compared lines). Create 47 ms and extract 87 ms on arm64; 87 ms and 165 ms on amd64 (loopback, page cache, one run; not a performance claim). Parent directories that are not in the archive (`home/` for `home/agent`) are not restored with their metadata. |
| 4a Root, 7 capabilities | PASS both. Restore with `--same-owner` of test 3: exit 0, 0 bytes on stderr. |
| 4b Extended attributes | A `security.*` entry, default flags, 7 capabilities: `Cannot restore extended attributes: security.sbxtest`, exit 1, the file is written. 4c: with `--no-xattrs --no-acls --no-fflags` the same tar exits 0 with no output. |
| 4d Unmapped id | Root of a nested user namespace with one mapped id, tar owner 1000: `Can't set user=1000/group=1000 ...: Invalid argument`, exit 1, the file is written and owned by root of the namespace. |
| 5 End of input | A complete tar, then the writer keeps the pipe open for 10 s: `bsdtar -x -f -` exits 0 after 10.0 s (arm64 10 016 ms, amd64 10 074 ms): it waits for end of input, it does not stop at the end-of-archive blocks. Truncated inside a file: exit 1, `Truncated tar archive detected while reading data`, also with the pipe held open (it returns when the writer ends). Tar without its end-of-archive blocks: exit 0. Empty input: exit 0. |
| 6 Names | Raw bytes (`\377\376`, Latin-1), `café`, Japanese, emoji, a directory with a raw byte. Default options, locale C and C.UTF-8: names equal, exit 0, but stderr holds `Can't translate pathname ... to UTF-8` (in C also for valid UTF-8). `--options hdrcharset=BINARY` on create: names equal, exit 0, silent, in both locales. The option on extract: exit 1, names lost (`iconv_open failed: Cannot handle ``BINARY''` / `A character-set conversion not fully supported`). |
| 7 `pause` as pid 1 | PASS both. Control with `sleep infinity` as pid 1: the orphan stays `State: Z`. With `pause`: pid gone, 0 zombies. SIGTERM: exit 0 after 222 ms (arm64), 170 ms (amd64). `docker stop -t 10`: 219 ms, exit 0 (arm64), 147 ms (amd64). Resident memory of pid 1: 24 kB on arm64; on amd64 the emulator is pid 1 and the value (4 916 kB) is not judged. |
| 8 Special names | PASS both. Names with a space, a tab and a newline; two hard links share one inode after restore; a 0-byte file stays 0 bytes. |
| 9 Non-root `--same-owner` | uid 10001, all capabilities dropped, a tar whose entries belong to 10001 and to other ids: exit 1 with `Can't set user=0/group=0 for work: Operation not permitted` (one line per foreign entry); every entry is still written, owned by 10001. A tar whose entries all belong to 10001: exit 0. The same tar with `--no-same-owner`: exit 0, no output. |
| 10 Second container | PASS both. The tools image with `--read-only --user 0`, the 7 capabilities, command `/sbx/bin/pause`; `docker exec -i /sbx/bin/bsdtar -x -f - --preserve-permissions --same-owner --numeric-owner -C /shared` exit 0, no stderr; owners 0 and 10001 and modes 0440, 0444, 0555 kept; a write to the root file system fails. A probe container in the same pid namespace: its orphan is reaped (gone after 4 s). SIGTERM: exit 0 after 273 ms (arm64), 181 ms (amd64). |
| 10b Read side as non-root | PASS both. uid 10001, no capability, `create` on root-owned `0644` and `0755` entries: exit 0. With a `0600` file of root: `Can't open ... Permission denied`, exit 1. |

The load average at the start of each run was between 4 and 9 (Mac with other work). The `RESULT` lines of the last run are in the step summary of the workflow run.
