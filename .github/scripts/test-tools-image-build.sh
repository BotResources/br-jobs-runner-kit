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

test_2_init_extraction() {
  local out
  docker volume create "$TOOLS_VOL" >/dev/null
  out=$(tools_run $SEVEN_CAPS -v "${TOOLS_VOL}:/opt/sbx" "$IMAGE" \
    /sbx/bin/bsdtar -x -p --no-same-owner -f /sbx/tools.tar -C /opt/sbx 2>&1)
  local init_code=$?
  local again
  again=$(tools_run $SEVEN_CAPS -v "${TOOLS_VOL}:/opt/sbx" "$IMAGE" \
    /sbx/bin/bsdtar -x -p --no-same-owner -f /sbx/tools.tar -C /opt/sbx 2>&1)
  local again_code=$?
  local script='
set -u
for p in bsdtar pause; do
  m=$(stat -c %a /opt/sbx/bin/$p); o=$(stat -c %u:%g /opt/sbx/bin/$p)
  echo "INFO: $p mode=$m owner=$o"
  [ "$m" = 555 ] || echo "FAIL: $p mode $m"
done
echo "INFO: bin dir mode=$(stat -c %a /opt/sbx/bin) VERSION=$(cat /opt/sbx/VERSION)"
ls -A /opt/sbx | tr "\n" " " | sed "s/^/INFO: top: /"; echo
[ "$(ls -A /opt/sbx | tr "\n" " ")" = "VERSION bin " ] || echo "FAIL: unexpected top-level entries"
/opt/sbx/bin/bsdtar --version | head -n 1 | sed "s/^/INFO: probe image runs: /"
echo "INFO: pause in probe image: $(timeout 1 /opt/sbx/bin/pause; echo rc=$?)"
touch /opt/sbx/x 2>&1 | sed "s/^/INFO: write on ro mount: /"
'
  local output code
  output=$(probe_script "$script" 2>&1)
  code=$?
  if [ "$init_code" -ne 0 ] || [ "$again_code" -ne 0 ] || [ -n "$out$again" ]; then
    output="${output}"$'\n'"FAIL: init extraction exit=${init_code}/${again_code} stderr=${out}${again}"
  fi
  verdict_of_script 2 "init extraction as a pod does it, read-only mount in a second image" "$output" "$code"
}
