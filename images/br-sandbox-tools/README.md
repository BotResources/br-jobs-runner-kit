# br-sandbox-tools

The tools that every sandbox pod needs, in one `FROM scratch` image: a static `bsdtar` (libarchive) and `pause`.

Registry path: `ghcr.io/botresources/br-sandbox-tools`, built by `.github/workflows/images.yml` (see [`../README.md`](../README.md) for the push and tag policy), one manifest list for `linux/amd64` and `linux/arm64`. A deployment pins the digest of the list.

## Content

| Path | What | Mode |
|---|---|---|
| `/sbx/bin/bsdtar` | libarchive 3.8.11, `bsdtar` only, static, musl | 0555 |
| `/sbx/bin/pause` | our own reaper (`pause.c`, 44 lines), static, musl | 0555 |
| `/sbx/tools.tar` | pax tar of `bin/`, `bin/bsdtar`, `bin/pause`, `VERSION`; owner 0:0; `bin/` is 0755 | 0644 |
| `/sbx/VERSION` | `libarchive <ver>; musl <ver>; pause <ver>; rev <git sha>` | 0444 |
| `/sbx/LICENSES/` | `libarchive-COPYING`, `musl-COPYRIGHT`, `br-sandbox-tools-Apache-2.0` | 0444 |

No shell, no `cp`, and nothing outside `/sbx` (test 11). Size: about 1 MB per architecture (limit 5 MB). OCI labels: `source`, `version`, `revision`, `licenses`, `title`.

## Contract

Mount path: a chart value, default `/opt/sbx`. Programs: `<mountPath>/bin/bsdtar`, `<mountPath>/bin/pause`.

Init container (copy without a shell). Test 12 runs it as uid 10001 with every capability dropped and `no-new-privileges`, on a volume that this user owns; the volume must be writable by the user that runs the init container:

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

1. **Exit rule.** Exit 0 means every entry is done and nothing was reported. Any other exit fails the step. Warnings (a failed `chown`, a refused xattr, a file that cannot be read) give exit 1, except the skipped socket of rule 10, which gives exit 0 and a message on stderr; the entries that could be written are on disk.
2. **End of input.** `bsdtar -x -f -` does not stop at the end-of-archive blocks: it waits for end of input on stdin. The caller must close stdin after the last byte of the tar. A writer that keeps the pipe open holds the process for as long as it stays open.
3. **Empty input exits 0** and extracts nothing. The caller checks that it sent bytes.
4. **Truncated tar** (cut inside a file) exits 1 with `Truncated tar archive detected while reading data`. A tar that only lacks its end-of-archive blocks exits 0.
5. **File names.** `--options hdrcharset=BINARY` goes on create only. On extract the option makes `bsdtar` exit 1 (`iconv_open failed: Cannot handle ``BINARY''`) and names are not restored. Without the option on create, non-UTF-8 names give a `Can't translate pathname ... to UTF-8` warning on stderr with exit 0; names still arrive byte for byte.
6. **Owner.** `--same-owner` needs `CHOWN` (root with the 7-capability set). As uid 10001 with no capability it exits 1 for each entry that belongs to another id, and still writes the entry as the caller. A tar whose entries all belong to the caller exits 0. An id that is not mapped in the user namespace gives `Can't set user=..: Invalid argument` and exit 1; the file is written.
7. **Extended attributes.** Without `--no-xattrs`, a `security.*` entry in the tar gives `Cannot restore extended attributes` and exit 1. The flags `--no-xattrs --no-acls --no-fflags` are part of every call above.
8. **Read side.** `create` as a non-root user on a directory of root: files that are readable are stored; a `0600` file of root gives ``Can't open `<file>': Permission denied`` and a `0700` directory of root gives `<dir>: Couldn't visit directory: Permission denied`; both exit 1.
9. **Second container.** The seed is written by `pods/exec` into a container that runs `/sbx/bin/pause`, as root of the user namespace, with `readOnlyRootFilesystem: true`. `docker exec -i /sbx/bin/bsdtar -x -f - ... -C <shared volume>` works; `bsdtar` writes only to the volume.

10. **Special files and holes.** A socket in a saved path is skipped: stderr gets one line `<path>: <path>: pax format cannot archive sockets`, the exit code is 0, and the tar has no entry for it. A named pipe and a device node are stored as entries. A restore as root with the 7 capabilities writes the pipe and refuses the device node (`Can't create '<path>': Operation not permitted`, exit 1, every other entry written), because `mknod` needs `CAP_MKNOD`. A sparse file is stored with its holes and restored sparse when the file system of the source keeps holes: a 10 MiB file with one data byte gives a tar under 1 MiB, and the restored file is 10 MiB long with the last byte kept.
11. **One file.** `create` of one path stores that one entry and no parent directory; `extract` with the name of one entry writes that entry and the directories above it, and nothing else. Both exit 0 with no output.

`pause`: pid 1 sleeps, reaps every child on SIGCHLD (`waitpid(-1, WNOHANG)` in a loop, and once at start), and exits 0 at once on SIGTERM or SIGINT. It reads no argument and no input and prints nothing.

## Why

| Thing | Why it is the way it is |
|---|---|
| `FROM scratch`, no shell | The sandbox needs two programs. A shell or busybox adds attack surface and GPL duties. |
| `bsdtar` and not GNU tar | libarchive is BSD-2-Clause, so no product in the image is copyleft; only the GCC runtime parts of the Licences section are, under their exception. GNU tar is GPL. |
| musl, `-all-static -no-pie` | glibc static brings LGPL duties. `-all-static` is needed: libtool drops a plain `-static`, which gives a binary with an interpreter that `scratch` does not have. `-no-pie` keeps the program free of a dynamic section, so the test "no `PT_INTERP`, no `PT_DYNAMIC`" is exact. |
| `--without` for every codec and crypto library | The wire is raw tar; zstd runs in the runner. `libb2/bundled` in the version line is the BLAKE2 code that libarchive carries in its own source; it links nothing. |
| Our own `pause.c` | 44 lines, one behaviour, no upstream copy and no NOTICE duty. The reaper loop runs once at start, so a child that died before the handler was set does not stay a zombie. |
| `--no-same-owner` in the init command | The init container extracts files for the user that runs it. The programs are mode 0555, owned by that user. |
| `bin/` is 0755 in the tar | A second extraction by a non-root init user must be able to replace the files in `bin/`. Test 12 runs the second extraction as uid 10001, and a control shows that a tar with `bin/` at 0555 fails the second extraction with `Permission denied`. |
| Fixed mtime 1 and a fixed entry list in `tools.tar` | The tar carries no build time. |
| The sources are pinned by sha256 | `ADD --checksum` in the `Dockerfile` fails the build when the libarchive 3.8.11 tarball (GitHub release) or the musl 1.2.6 tarball does not match. To repeat the check, download each URL of the `Dockerfile` and run `sha256sum` on it. |
| The musl tarball gives only the licence text | `COPYRIGHT` of the tarball becomes `/sbx/LICENSES/musl-COPYRIGHT`. The linked C library is the static library of the Alpine package `musl-dev`; the build fails unless the packages `musl` and `musl-dev` are at the version `MUSL_VERSION` of that tarball, so the licence text and the linked library are the same release. |
| The base image is pinned by digest, the compiler packages are not | The digest fixes the Alpine 3.24.4 image that the build starts from. `apk add build-base` takes the compiler and the other packages from the Alpine 3.24 index when the build runs, without a version pin, so two builds of the same commit can use a newer compiler package. The binaries are checked by what they are (test 1), not by a reproducible build. A new libarchive or musl CVE means a new pin and a new image. |
| No `provenance` and no `sbom` in the workflow | They add `unknown/unknown` entries to the manifest list. The list holds the two platforms only. |
| `hdrcharset=BINARY` on create only | Measured: on extract it fails in this build. |

## Licences

| Product | Version | SPDX | In the image |
|---|---|---|---|
| libarchive | 3.8.11 | BSD-2-Clause (some files public domain, CC0, Apache-2.0 or OpenSSL licence, see `COPYING`) | `bsdtar` statically linked; text in `/sbx/LICENSES/libarchive-COPYING` |
| musl libc | 1.2.6 | MIT | statically linked into both programs; text in `/sbx/LICENSES/musl-COPYRIGHT` |
| `pause.c`, Dockerfile, CI | n/a | Apache-2.0 | `/sbx/LICENSES/br-sandbox-tools-Apache-2.0` |
| GCC runtime: `crtbeginT.o`, `crtend.o`, and on `linux/arm64` 16 members of `libgcc.a` | 15.2.0 (Alpine `gcc`) | GPL-3.0-or-later WITH GCC-exception-3.1 | statically linked, see below; no text in the image |

No copyleft product is built from source: libarchive, musl and our files are permissive, and libacl (LGPL-2.1) is not built in (`--disable-acl`). The one copyleft-licensed code in the binaries is the GCC runtime library code that `gcc` 15.2.0 links into every static program. Measured on the shipped files (the build of this `Dockerfile` without the two `strip` calls gives files that are byte-identical to the shipped ones after `strip`):

- Both programs on both architectures hold the GCC start-up objects `crtbeginT.o` and `crtend.o` (`frame_dummy`, `__do_global_dtors_aux`, `deregister_tm_clones`, `__EH_FRAME_BEGIN__`) and the `.comment` entry `GCC: (Alpine 15.2.0) 15.2.0`.
- `linux/arm64`: `bsdtar` links 16 members of `libgcc.a`: the quad-precision soft-float helpers behind `long double` (`addtf3`, `subtf3`, `multf3`, `divtf3`, `eqtf2`, `getf2`, `letf2`, `extenddftf2`, `extendsftf2`, `trunctfdf2`, `trunctfsf2`, `fixtfsi`, `fixunstfsi`, `floatsitf`, `floatunsitf`, `sfp-exceptions`). `pause` links none.
- `linux/amd64`: neither program links a member of `libgcc.a`.
- On both architectures no `_Unwind_*` symbol is present: `libgcc_eh.a` is not linked.

These parts are GPL-3.0-or-later WITH GCC-exception-3.1 (GCC Runtime Library Exception). The exception lets the object code be propagated under terms of our choice when it is built by an eligible compilation process; the `Dockerfile` runs plain `gcc` with no plug-in. We distribute that object code inside binaries under the licences of the table, and no source offer is required. To repeat the measure, link without `strip`, then compare `nm` of each program with `nm --defined-only "$(gcc -print-libgcc-file-name)"`.

## Build

```
docker buildx build --platform linux/arm64 --build-arg REVISION=$(git rev-parse HEAD) \
  --label org.opencontainers.image.revision=$(git rev-parse HEAD) --load \
  -t br-sandbox-tools:test -f images/br-sandbox-tools/Dockerfile .
bash .github/scripts/test-tools-image.sh br-sandbox-tools:test linux/arm64
```

The workflow runs the same script on each architecture. When it pushes, it pushes each platform image by digest first, pulls that digest, and runs the label check and these tests on the pulled image; the tag and the manifest list are created only when both platforms passed (see [`../README.md`](../README.md)). The tests need Docker, `python3` and a pull of `debian:bookworm-slim` (by digest) as the sandbox-like image. The save, restore and init command lines that the tests run are read from this README, so a change of a line here is a change of the tested line.

## Tests

Each test prints one `RESULT` line. A test passes only when it ran at least one assertion and reached its end; a failed assertion fails the test, and a test that could not run any assertion on its subject is `SKIP` (named in the line), never `PASS`; an assertion that is skipped inside a test that ran others is named in the line as `SKIP:`. Numbers (timings, sizes) are in `INFO` lines and judge nothing, except the size limit of 1b and the resident memory of 7.

| Test | What it asserts |
|---|---|
| 1 | `bsdtar` and `pause` have no `PT_INTERP` and no `PT_DYNAMIC`; the version line is `bsdtar 3.8.x - libarchive 3.8.x ...` and lists no compression, crypto, XML or ACL library. |
| 1b | The image is under 5 MB (about 1 MB per architecture). |
| 11 | `/sbx` holds `LICENSES`, `VERSION`, `bin`, `tools.tar`; `/sbx/bin` holds `bsdtar` and `pause` at 0555; the three licence texts exist, are mode 0444 and hold their marker text; `VERSION` has the documented format and its `rev` equals the `revision` label; `tools.tar` holds `bin`, `bin/bsdtar`, `bin/pause`, `VERSION` in that order with owner 0:0, mtime 1 and the documented modes, with the same bytes as `/sbx`; the container file system holds nothing outside `/sbx` except the runtime mount points; there is no shell; `bsdtar` runs by its absolute path. |
| 2 | The init command, run as root with the 7 capabilities of the sandbox pod, exits 0 with no output, twice. A second image mounts the volume read-only and runs both programs; modes, owners, the entries of `bin/` and of the top level are as documented; a write on the mount fails with `Read-only file system`. |
| 2b | Root with the 7 capabilities: `mount -o remount,rw /opt/sbx` fails with a permission error, the mount stays `ro`, a write still fails with `Read-only file system`. A control as a privileged root remounts it read-write, so the assertion can fail. |
| 12 | The init command, run as uid 10001 with every capability dropped and `no-new-privileges` on a volume that user owns, exits 0 with no output, twice; files are owned by 10001; a control extraction of a tar with `bin/` at 0555 exits 1 with `Permission denied` the second time. |
| 3 | Save and restore with the lines of the contract: 1 000 files, one 50 MB file, 4 symlinks (one dangling), empty directories with modes and ids, executable bits, a sub-second mtime. Both exit 0 with no output; mode, uid, gid, symlink targets, SHA-256 of every file, mtime to the second and the sub-second mtime are equal. |
| 4a | The restore of test 3 as root with the 7 capabilities: exit 0, no output. |
| 4b | A `security.*` xattr in the tar, restore without `--no-xattrs --no-acls --no-fflags`: exit 1, `Cannot restore extended attributes`, the file is written. |
| 4c | The same tar with the three `--no-*` flags: exit 0, no output. |
| 4d | A tar owner id that is unmapped in a nested user namespace: exit 1, `Can't set user=1000/group=1000 ...: Invalid argument`, the file is written. `SKIP` on a platform that the daemon emulates and that cannot create a user namespace (a foreign architecture under QEMU); on a native platform a user namespace that cannot be created fails the test. |
| 5 | `bsdtar -x -f -` is still running 3 s after the last entry was extracted while the pipe is open, and ends with exit 0 once the pipe is closed; empty input exits 0 with no output and extracts nothing; a tar cut inside a file exits 1 with `Truncated tar archive detected while reading data`, with the pipe held open and closed; a tar with only its end-of-archive blocks cut exits 0. |
| 6 | Raw-byte and Unicode names. Default options, locale C and C.UTF-8: exit 0, names equal, the `Can't translate pathname ... to UTF-8` warning. `hdrcharset=BINARY` on create: exit 0, names equal, no output, in both locales. The option on extract: exit 1, `iconv_open failed: Cannot handle ``BINARY''`, names not restored. |
| 8 | Names with a space, a tab and a newline; two hard links share one inode after restore; a 0-byte file stays 0 bytes; save and restore with the lines of the contract exit 0 with no output. |
| 7 | A control with `sleep infinity` as pid 1 leaves its orphan a zombie; with `pause` the orphan is gone and no zombie remains; `SIGTERM`, `SIGINT` and `docker stop -t 10` end `pause` with exit 0; the resident memory of pid 1 is under 1 MiB (judged only on a platform the daemon runs natively). |
| 7b | `pause` started with the arguments `--version -h an-argument` and bytes on stdin, which then reaches its end: it keeps running, reaps an orphan, ends with exit 0 on `SIGTERM`, and nothing was printed on stdout or stderr. |
| 9 | uid 10001, no capability. A tar with entries of other ids: `--same-owner` exits 1 with one `Can't set user=` message for each foreign entry (3), and every entry (6 paths) is written, owned by 10001. A tar of own ids exits 0. `--no-same-owner` on the first tar exits 0 with no output. |
| 10 | The tools image with `--read-only --user 0`, the 7 capabilities and `/sbx/bin/pause`: `docker exec -i bsdtar -x` into a shared volume exits 0 with no output, with owners 0 and 10001 and modes 0440, 0444 and 0555 kept; a control write to the volume works and a write to the root file system fails; an orphan of a probe container in the same pid namespace is reaped; `SIGTERM` ends the container with exit 0. |
| 10b | uid 10001, no capability, `create` on root-owned `0644` and `0755` entries: exit 0, no output. A directory with a `0600` file of root, and a `0700` directory of root: exit 1 with `Permission denied`. |
| 10c | `create` of `work/a.txt` gives a tar of exactly that entry, exit 0, no output; `extract` of `work/b.txt` from a tar of two files writes that file and its directory only, with its content and mode, exit 0, no output. |
| 13 | Rule 10: a tree with a socket, a fifo, a device node, a regular file and a 10 MiB sparse file. Save exits 0 with the socket line on stderr; the socket is not in the tar, the other four are; the tar is under 1 MiB (judged only when the source file system keeps holes). Restore as root with the 7 capabilities exits 1 for the device node, writes the other entries, and the sparse file is again 10 MiB long, sparse, with its last byte. |

The script fails when a test of the list `EXPECTED_TEST_IDS` in `.github/scripts/test-tools-image-gate.sh` has no `RESULT` line (also when the script ends early), and when a test that ran is not in the list. The `RESULT` lines of a run are in the step summary of the workflow run for each image and platform.
