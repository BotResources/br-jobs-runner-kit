#!/bin/sh
set -eu

image="${1:?usage: smoke.sh <image reference>}"
uid=12345

run() {
  docker run --rm --user "${uid}:${uid}" --cap-drop ALL --security-opt no-new-privileges "$image" "$@"
}

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

check() {
  name="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "ok   $name"
  else
    fail "$name"
  fi
}

check "uid is ${uid} with no passwd entry" run sh -c "[ \"\$(id -u)\" = ${uid} ] && ! getent passwd ${uid}"
check "/bin/sh exists" run test -x /bin/sh
check "cat exists" run sh -c 'command -v cat'
check "find exists" run sh -c 'command -v find'
check "ps exists" run sh -c 'command -v ps'
check "HOME is /home/agent, workdir is /work" run sh -c "[ \"\$HOME\" = /home/agent ] && [ \"\$(pwd)\" = /work ]"
check "/work and /home/agent are writable" run sh -c 'touch /work/.w /home/agent/.w'
check "python is 3.12" run python -c 'import sys; assert sys.version_info[:2] == (3, 12)'
check "libraries import" run python -c 'import pandas, openpyxl, docx, pptx, setuptools'
check "pandas reads and writes xlsx" run python -c '
import pandas as pd
pd.DataFrame({"a": [1, 2]}).to_excel("/work/t.xlsx", index=False)
assert pd.read_excel("/work/t.xlsx")["a"].tolist() == [1, 2]
'

convert() {
  kind="$1"
  name="$2"
  check "soffice converts ${kind} to pdf" run sh -c "
    python - <<'PY'
import docx, openpyxl, pptx
docx_doc = docx.Document()
docx_doc.add_paragraph('smoke')
docx_doc.save('/work/s.docx')
book = openpyxl.Workbook()
book.active['A1'] = 'smoke'
book.save('/work/s.xlsx')
deck = pptx.Presentation()
deck.slides.add_slide(deck.slide_layouts[0]).shapes.title.text = 'smoke'
deck.save('/work/s.pptx')
PY
    soffice --headless --convert-to pdf --outdir /work /work/${name} >/dev/null 2>&1
    head -c 4 /work/s.pdf | grep -q '%PDF'
  "
}

convert docx s.docx
convert xlsx s.xlsx
convert pptx s.pptx

echo "smoke passed: $image"
