import base64
import binascii
import os
import re
import sys

CANARY = os.environ["SCAN_CANARY"]
KEYLINE = os.environ.get("SCAN_KEYLINE", "").encode()
RAW = CANARY.encode()
PATTERNS = {
    "raw": RAW,
    "hex": binascii.hexlify(RAW),
    "urlenc": CANARY.replace("-", "%2D").encode(),
    "b64_0": base64.b64encode(RAW)[:-4],
    "b64_1": base64.b64encode(b"\x00" + RAW)[2:-4],
    "b64_2": base64.b64encode(b"\x00\x00" + RAW)[3:-4],
}


def hits(buffer):
    return sum(buffer.count(p) for p in PATTERNS.values() if p)


def key_hits(buffer):
    return buffer.count(KEYLINE) if KEYLINE else 0


def proc_pids():
    own = str(os.getpid())
    return [p for p in os.listdir("/proc") if p.isdigit() and p != own]


def scan_environ():
    found = key = 0
    for pid in proc_pids():
        try:
            data = open("/proc/%s/environ" % pid, "rb").read()
        except OSError:
            continue
        found += hits(data)
        key += key_hits(data)
    return found, key


def scan_memory():
    found = key = 0
    for pid in proc_pids():
        try:
            maps = open("/proc/%s/maps" % pid).read().splitlines()
            mem = open("/proc/%s/mem" % pid, "rb", 0)
        except OSError:
            continue
        for line in maps:
            match = re.match(r"([0-9a-f]+)-([0-9a-f]+) (\S+)", line)
            if not match or "r" not in match.group(3):
                continue
            start, end = int(match.group(1), 16), int(match.group(2), 16)
            if end - start > 512 * 1024 * 1024:
                continue
            try:
                mem.seek(start)
                data = mem.read(end - start)
            except (OSError, ValueError, OverflowError):
                continue
            found += hits(data)
            key += key_hits(data)
    return found, key


def scan_files():
    found = key = 0
    for root, dirs, files in os.walk("/", followlinks=False):
        if root.startswith(("/proc", "/sys", "/dev")):
            dirs[:] = []
            continue
        for name in files:
            path = os.path.join(root, name)
            try:
                if os.path.islink(path) or not os.path.isfile(path):
                    continue
                if os.path.getsize(path) > 200 * 1024 * 1024:
                    continue
                data = open(path, "rb").read()
            except OSError:
                continue
            found += hits(data)
            key += key_hits(data)
    return found, key


def scan_file(path):
    data = open(path, "rb").read()
    return hits(data), key_hits(data)


if sys.argv[1] == "file":
    results = {"file": scan_file(sys.argv[2])}
else:
    results = {"env": scan_environ(), "mem": scan_memory(), "fs": scan_files()}
for place, (found, key) in results.items():
    print("%s value_hits=%d key_hits=%d" % (place, found, key))
