#!/usr/bin/env bash

test_1_static_and_version() {
  local dir="${WORK_DIR}/bins" cid version_out first bad
  mkdir -p "$dir"
  cid=$(docker create --platform "$PLATFORM" "$IMAGE" /sbx/bin/pause)
  docker cp "${cid}:/sbx/bin/bsdtar" "${dir}/bsdtar" >/dev/null
  docker cp "${cid}:/sbx/bin/pause" "${dir}/pause" >/dev/null
  docker rm "$cid" >/dev/null
  local verdict=PASS detail="" program
  for program in bsdtar pause; do
    local report
    report=$(python3 -I - "${dir}/${program}" <<'PY'
import struct, sys
data = open(sys.argv[1], "rb").read()
assert data[:4] == b"\x7fELF" and data[4] == 2
phoff, = struct.unpack_from("<Q", data, 0x20)
phentsize, phnum = struct.unpack_from("<HH", data, 0x36)
types = [struct.unpack_from("<I", data, phoff + i * phentsize)[0] for i in range(phnum)]
print("interp" if 3 in types else "static", "dynamic" if 2 in types else "nodynamic", len(data))
PY
)
    detail="${detail}${program}: ${report}; "
    case "$report" in "static nodynamic "*) ;; *) verdict=FAIL ;; esac
  done
  version_out=$(tools_run "$IMAGE" /sbx/bin/bsdtar --version 2>&1)
  first=$(head -n 1 <<<"$version_out")
  detail="${detail}version: ${version_out//$'\n'/ / }; "
  if ! [[ "$first" =~ ^bsdtar\ 3\.8\.[0-9]+\ -\ libarchive\ 3\.8\.[0-9]+(\ libb2/bundled)?\ ?$ ]]; then
    verdict=FAIL
  fi
  bad=$(grep -i -E 'zlib|bz2lib|liblzma|libzstd|liblz4|openssl|nettle|mbedtls|libxml|expat|libacl|libcrypto' <<<"$version_out" || true)
  if [ -n "$bad" ]; then
    verdict=FAIL
    detail="${detail}forbidden library listed: ${bad}; "
  fi
  record "$verdict" 1 "static binaries and version line" "$detail"
}

test_1b_image_size() {
  local size
  size=$(docker image inspect "$IMAGE" --format '{{.Size}}')
  if [ "$size" -lt 5242880 ]; then
    record PASS 1b "image size under 5 MB" "${size} bytes"
  else
    record FAIL 1b "image size under 5 MB" "${size} bytes"
  fi
}
