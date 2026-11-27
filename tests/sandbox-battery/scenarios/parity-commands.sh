export PIP_DISABLE_PIP_VERSION_CHECK=1
work=$(mktemp -d)
cd "$work" || exit 9
echo hello > f.txt
chmod 750 f.txt
stat -c '%a' f.txt
(sleep 30 & echo $! > bg.pid)
sleep 0.3
bg=$(cat bg.pid)
kill -0 "$bg" && echo "background program alive"
echo "sleep processes: $(grep -l '^sleep$' /proc/[0-9]*/comm 2> /dev/null | wc -l | tr -d ' ')"
echo "cmdline: $(tr '\0' ' ' < "/proc/$bg/cmdline")"
echo "pipe lines: $(printf 'a\nb\nc\n' | wc -l | tr -d ' ')"
kill -TERM "$bg"
sleep 0.5
if kill -0 "$bg" 2> /dev/null; then echo "after TERM: alive"; else echo "after TERM: gone"; fi
sh -c 'exit 3'
echo "exit code: $?"
python3 - << 'PY'
import zipfile
files = {
    "parpkg/__init__.py": "X = 1\n",
    "parpkg-0.1.dist-info/METADATA": "Metadata-Version: 2.1\nName: parpkg\nVersion: 0.1\n",
    "parpkg-0.1.dist-info/WHEEL": "Wheel-Version: 1.0\nGenerator: parity\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
    "parpkg-0.1.dist-info/RECORD": "parpkg-0.1.dist-info/RECORD,,\n",
}
with zipfile.ZipFile("parpkg-0.1-py3-none-any.whl", "w") as wheel:
    for name, content in files.items():
        wheel.writestr(name, content)
PY
python3 -m pip install --no-index --no-deps -q --root-user-action=ignore ./parpkg-0.1-py3-none-any.whl > pip.log 2>&1
echo "pip exit: $?"
python3 -c 'import parpkg; print("import ok", parpkg.X)'
