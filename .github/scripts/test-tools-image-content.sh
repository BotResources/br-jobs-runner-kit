#!/usr/bin/env bash

IMAGE_CONTENT_CHECKS='
import hashlib, os, re, stat, sys, tarfile

sbx, rootfs, label_revision = sys.argv[1], sys.argv[2], sys.argv[3]

def pas(text):
    print("PASS: " + text)

def fail(text):
    print("FAIL: " + text)

def eq(label, got, wanted):
    if got == wanted:
        pas(label)
    else:
        fail("{}: wanted {!r} got {!r}".format(label, wanted, got))

def mode_of(path):
    return stat.S_IMODE(os.stat(path).st_mode)

def read(path):
    with open(path, "rb") as handle:
        return handle.read()

eq("entries of /sbx", sorted(os.listdir(sbx)), ["LICENSES", "VERSION", "bin", "tools.tar"])
eq("entries of /sbx/bin", sorted(os.listdir(sbx + "/bin")), ["bsdtar", "pause"])
for program in ("bsdtar", "pause"):
    eq("mode of /sbx/bin/" + program, oct(mode_of(sbx + "/bin/" + program)), oct(0o555))
eq("mode of /sbx/VERSION", oct(mode_of(sbx + "/VERSION")), oct(0o444))
eq("mode of /sbx/tools.tar", oct(mode_of(sbx + "/tools.tar")), oct(0o644))

licenses = {
    "libarchive-COPYING": b"The libarchive distribution as a whole is Copyright by Tim Kientzle",
    "musl-COPYRIGHT": b"musl as a whole is licensed under the following standard MIT license",
    "br-sandbox-tools-Apache-2.0": b"Apache License\n                           Version 2.0, January 2004",
}
eq("entries of /sbx/LICENSES", sorted(os.listdir(sbx + "/LICENSES")), sorted(licenses))
for name, marker in licenses.items():
    path = sbx + "/LICENSES/" + name
    text = read(path)
    if marker in text and len(text) > 500:
        pas("text of " + name)
    else:
        fail("text of {} lacks {!r} or is shorter than 500 bytes ({} bytes)".format(name, marker, len(text)))
    eq("mode of " + name, oct(mode_of(path)), oct(0o444))

version = read(sbx + "/VERSION").decode()
shape = r"libarchive 3\.8\.[0-9]+; musl 1\.2\.[0-9]+-r[0-9]+; pause [0-9]+; rev ([0-9a-f]{40})\n"
found = re.fullmatch(shape, version)
if found:
    pas("format of VERSION")
    if label_revision:
        eq("revision in VERSION equals the revision label", found.group(1), label_revision)
    else:
        print("INFO: the image has no revision label; VERSION holds " + found.group(1))
else:
    fail("format of VERSION: {!r}".format(version))

expected = [("bin", 0o755, tarfile.DIRTYPE), ("bin/bsdtar", 0o555, tarfile.REGTYPE), ("bin/pause", 0o555, tarfile.REGTYPE), ("VERSION", 0o444, tarfile.REGTYPE)]
with tarfile.open(sbx + "/tools.tar") as archive:
    members = archive.getmembers()
    eq("entries of tools.tar, in order", [m.name for m in members], [e[0] for e in expected])
    eq("type and mode of the entries of tools.tar", [(m.type, m.mode) for m in members], [(e[2], e[1]) for e in expected])
    eq("owner of the entries of tools.tar", {(m.uid, m.gid, m.uname, m.gname) for m in members}, {(0, 0, "", "")})
    eq("mtime of the entries of tools.tar", {m.mtime for m in members}, {1})
    for name, path in (("bin/bsdtar", "/bin/bsdtar"), ("bin/pause", "/bin/pause"), ("VERSION", "/VERSION")):
        content = archive.extractfile(name).read()
        eq("tools.tar holds the same bytes as /sbx" + path, hashlib.sha256(content).hexdigest(), hashlib.sha256(read(sbx + path)).hexdigest())

runtime = {".dockerenv", "dev", "etc", "proc", "sys", "run", "etc/hostname", "etc/hosts", "etc/resolv.conf", "etc/mtab"}
outside = []
with tarfile.open(rootfs) as archive:
    for member in archive.getmembers():
        name = member.name.rstrip("/")
        if name == "sbx" or name.startswith("sbx/") or name in runtime or name.startswith("dev/"):
            continue
        outside.append(name)
eq("entries of the container file system outside /sbx and the runtime mount points", outside, [])
'

test_11_body() {
  local cid dir="${WORK_DIR}/image" revision code
  mkdir -p "$dir"
  revision=$(docker image inspect "$IMAGE" --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}')
  cid=$(docker create --platform "$PLATFORM" "$IMAGE" /sbx/bin/pause)
  docker cp "${cid}:/sbx" "${dir}/" >/dev/null
  docker export "$cid" >"${WORK_DIR}/rootfs.tar"
  docker rm "$cid" >/dev/null
  python3 -I -c "$IMAGE_CONTENT_CHECKS" "${dir}/sbx" "${WORK_DIR}/rootfs.tar" "$revision"
  tools_run "$IMAGE" /bin/sh -c true >/dev/null 2>&1
  code=$?
  ne0 "the image has no shell" "$code"
  tools_run "$IMAGE" /sbx/bin/bsdtar --version >/dev/null 2>&1
  eq "the image runs bsdtar by its absolute path" "$?" 0
}

test_11_image_content() {
  run_host_test 11 "content of /sbx: programs, modes, licence texts, VERSION line, tools.tar entries, nothing else in the image" test_11_body
}
