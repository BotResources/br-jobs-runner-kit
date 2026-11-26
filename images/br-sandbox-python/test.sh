#!/bin/sh
set -eu

image="${1:?usage: test.sh <image reference> [platform]}"
platform="${2:-}"
results="${RESULTS_FILE:-/dev/null}"
uid=12345
other_user=54321:0
here=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
docs="${scratch}/docs"
ran="${scratch}/ran"
volume="br-sandbox-python-test-$$"
mkdir "$docs"
chmod 777 "$docs"
: >"$ran"
docker volume create "$volume" >/dev/null

cleanup() {
  rm -rf "$scratch"
  docker volume rm -f "$volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT

dk() {
  docker run --rm ${platform:+--platform "$platform"} --network none --cap-drop ALL --security-opt no-new-privileges "$@"
}

ctr() {
  dk --user "${uid}:${uid}" "$image" "$@"
}

ctr_docs() {
  dk --user "${uid}:${uid}" -v "${docs}:/docs:ro" "$image" "$@"
}

ctr_docs_rw() {
  dk --user "${uid}:${uid}" -v "${docs}:/docs" "$image" "$@"
}

ctr_read_only() {
  dk --user "${uid}:${uid}" --read-only --tmpfs /tmp --tmpfs /work --tmpfs /home/agent -v "${docs}:/docs:ro" "$image" "$@"
}

ctr_volume() {
  dk --user "${uid}:${uid}" -v "${volume}:/home/agent" -v "${docs}:/docs:ro" "$image" "$@"
}

ctr_other_user() {
  dk --user "$other_user" -v "${docs}:/docs:ro" "$image" "$@"
}

fail() {
  echo "FAIL: $1" | tee -a "$results" >&2
  exit 1
}

check() {
  name="$1"
  shift
  echo "$name" >>"$ran"
  if "$@" >/dev/null 2>&1; then
    echo "ok   $name" | tee -a "$results"
  else
    fail "$name"
  fi
}

normalize() {
  tr 'A-Z_' 'a-z-'
}

freeze_matches_requirements() {
  grep -v '^[[:space:]]*$' "${here}/requirements.txt" | normalize | sort >"${scratch}/required"
  ctr pip freeze --all | grep -v '^pip==' | normalize | sort | diff - "${scratch}/required"
}

no_network() {
  ctr python -c '
import socket, sys
try:
    socket.create_connection(("1.1.1.1", 53), timeout=3)
except OSError:
    sys.exit(0)
sys.exit(1)
'
}

make_documents='
import docx, openpyxl, pptx
document = docx.Document()
document.add_paragraph("smoke")
document.save("/docs/s.docx")
book = openpyxl.Workbook()
book.active["A1"] = "smoke"
book.save("/docs/s.xlsx")
deck = pptx.Presentation()
deck.slides.add_slide(deck.slide_layouts[0]).shapes.title.text = "smoke"
deck.save("/docs/s.pptx")
'

pdf_is_valid=$(
  cat <<'CHECK'
[ "$(head -c 4 /work/s.pdf)" = "%PDF" ] && grep -aq "%%EOF" /work/s.pdf && [ "$(wc -c </work/s.pdf)" -ge 1000 ]
CHECK
)

conversion() {
  printf '%s' "cp /docs/$1 /work/ && soffice --headless --convert-to pdf --outdir /work /work/$1 >/dev/null 2>&1 && ${pdf_is_valid}"
}

cat >"${scratch}/expected" <<EOF
uid is ${uid} with no passwd entry
getent exists
sh is found by command -v
/bin/sh exists
cat exists
find exists
ps exists
bsdtar is absent
pause is absent
no network inside the container
HOME is /home/agent, workdir is /work
/work and /home/agent are writable
/work and /home/agent are writable as uid 54321 with gid 0
python is 3.12
libraries import
installed packages equal requirements.txt
pip check passes
pandas reads and writes xlsx
documents are built
soffice converts docx to pdf
soffice converts xlsx to pdf
soffice converts pptx to pdf
soffice converts docx to pdf on a read-only root with tmpfs on /tmp, /work and /home/agent
soffice converts docx to pdf with an empty volume on /home/agent
soffice converts docx to pdf as uid 54321 with gid 0
EOF

check "getent exists" ctr sh -c 'command -v getent'
check "uid is ${uid} with no passwd entry" ctr sh -c "[ \"\$(id -u)\" = ${uid} ] && ! getent passwd ${uid}"
check "sh is found by command -v" ctr sh -c 'command -v sh'
check "/bin/sh exists" ctr test -x /bin/sh
check "cat exists" ctr sh -c 'command -v cat'
check "find exists" ctr sh -c 'command -v find'
check "ps exists" ctr sh -c 'command -v ps'
check "bsdtar is absent" ctr sh -c '! command -v bsdtar'
check "pause is absent" ctr sh -c '! command -v pause'
check "no network inside the container" no_network
check "HOME is /home/agent, workdir is /work" ctr sh -c "[ \"\$HOME\" = /home/agent ] && [ \"\$(pwd)\" = /work ]"
check "/work and /home/agent are writable" ctr sh -c 'touch /work/.w /home/agent/.w'
check "/work and /home/agent are writable as uid 54321 with gid 0" ctr_other_user sh -c 'touch /work/.w /home/agent/.w'
check "python is 3.12" ctr python -c 'import sys; assert sys.version_info[:2] == (3, 12)'
check "libraries import" ctr python -c 'import pandas, openpyxl, docx, pptx, setuptools'
check "installed packages equal requirements.txt" freeze_matches_requirements
check "pip check passes" ctr pip check
check "pandas reads and writes xlsx" ctr python -c '
import pandas as pd
pd.DataFrame({"a": [1, 2]}).to_excel("/work/t.xlsx", index=False)
assert pd.read_excel("/work/t.xlsx")["a"].tolist() == [1, 2]
'
check "documents are built" ctr_docs_rw python -c "$make_documents"
check "soffice converts docx to pdf" ctr_docs sh -c "$(conversion s.docx)"
check "soffice converts xlsx to pdf" ctr_docs sh -c "$(conversion s.xlsx)"
check "soffice converts pptx to pdf" ctr_docs sh -c "$(conversion s.pptx)"
check "soffice converts docx to pdf on a read-only root with tmpfs on /tmp, /work and /home/agent" ctr_read_only sh -c "$(conversion s.docx)"
check "soffice converts docx to pdf with an empty volume on /home/agent" ctr_volume sh -c "$(conversion s.docx)"
check "soffice converts docx to pdf as uid 54321 with gid 0" ctr_other_user sh -c "$(conversion s.docx)"

sort "$ran" >"${scratch}/ran.sorted"
sort "${scratch}/expected" >"${scratch}/expected.sorted"
if ! diff "${scratch}/expected.sorted" "${scratch}/ran.sorted" >&2; then
  fail "the checks that ran differ from the expected list"
fi

echo "smoke passed: $image"
