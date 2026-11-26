#!/usr/bin/env bash

test_5_end_of_input() {
  local script='
B=/opt/sbx/bin/bsdtar
mkdir -p /data/in /data/out5 /data/out_empty /data/out_trunc /data/out_noend && cd /data/in
head -c 1048576 /dev/urandom > one.bin; printf x > two.txt
$B --create -b 1 --file /data/full.tar --format pax --numeric-owner -C /data/in -- one.bin two.txt
size=$(stat -c %s /data/full.tar)
eq "the test tar is a multiple of 512 bytes" "$((size % 512))" 0
eq "the last 1024 bytes of the test tar are the end-of-archive blocks" "$(tail -c 1024 /data/full.tar | tr -d "\000" | wc -c | tr -d " ")" 0

mkfifo /data/fifo
$B -x -f - -C /data/out5 </data/fifo 2>/data/e1 &
pid=$!
exec 7>/data/fifo
cat /data/full.tar >&7
for _ in $(seq 1 300); do
  [ "$(stat -c %s /data/out5/two.txt 2>/dev/null)" = 1 ] && break
  sleep 0.1
done
eq "the last entry is extracted while the pipe is open" "$(stat -c %s /data/out5/two.txt 2>/dev/null)" 1
sleep 3
if kill -0 "$pid" 2>/dev/null; then pass "bsdtar still runs 3 s after the end-of-archive blocks while the pipe is open"; else fail "bsdtar ended before the pipe was closed"; fi
exec 7>&-
for _ in $(seq 1 300); do
  kill -0 "$pid" 2>/dev/null || break
  sleep 0.1
done
if kill -0 "$pid" 2>/dev/null; then
  fail "bsdtar did not end within 30 s after the pipe was closed"
  kill "$pid"
  wait "$pid" 2>/dev/null
else
  wait "$pid"; rc=$?
  eq "bsdtar ends with exit 0 once the pipe is closed" "$rc" 0
fi
eq "stderr of the complete stream" "$(cat /data/e1)" ""

$B -x -f - -C /data/out_empty </dev/null 2>/data/e2; rc=$?
eq "empty input: exit" "$rc" 0
eq "empty input: stderr" "$(cat /data/e2)" ""
eq "empty input: nothing extracted" "$(ls -A /data/out_empty | wc -l | tr -d " ")" 0

{ head -c $((size / 2)) /data/full.tar; sleep 3; } | $B -x -f - -C /data/out_trunc 2>/data/e3; rc=$?
eq "truncated inside a file, pipe held open: exit" "$rc" 1
has "truncated inside a file, pipe held open: message" "$(cat /data/e3)" "Truncated tar archive detected while reading data"
head -c $((size / 2)) /data/full.tar | $B -x -f - -C /data/out_trunc 2>/data/e4; rc=$?
eq "truncated inside a file, pipe closed: exit" "$rc" 1
has "truncated inside a file, pipe closed: message" "$(cat /data/e4)" "Truncated tar archive detected while reading data"

head -c $((size - 1024)) /data/full.tar | $B -x -f - -C /data/out_noend 2>/data/e5; rc=$?
eq "no end-of-archive blocks, pipe closed: exit" "$rc" 0
eq "no end-of-archive blocks, pipe closed: stderr" "$(cat /data/e5)" ""
eq "no end-of-archive blocks: both files extracted" "$(ls /data/out_noend | tr "\n" " ")" "one.bin two.txt "
'
  run_scripted_test 5 "bsdtar -x -f - waits for end of input, empty input exits 0, truncated tar exits 1, missing end blocks exit 0" "$script" $SEVEN_CAPS
}
