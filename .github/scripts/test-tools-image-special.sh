#!/usr/bin/env bash

test_13_body() {
  local setup='
rm -rf /data/sp13 /data/sp13r /data/sp13.tar
mkdir -p /data/sp13/work /data/sp13r
cd /data/sp13/work
truncate -s 10M sparse
printf x | dd of=sparse bs=1 seek=10485759 conv=notrunc 2>/dev/null
mkfifo fifo
mknod dev c 1 3
perl -MSocket -e "socket(S, PF_UNIX, SOCK_STREAM, 0); bind(S, sockaddr_un(q(sock))) or die q(bind)"
printf plain > plain.txt
eq "set-up: the socket exists" "$([ -S sock ] && echo yes)" yes
eq "set-up: the device node exists" "$([ -c dev ] && echo yes)" yes
eq "set-up: the sparse file is 10 MiB long" "$(stat -c %s sparse)" 10485760
pass "set-up done"
'
  local checks='
B=/opt/sbx/bin/bsdtar
if [ "$(( $(stat -c %b /data/sp13/work/sparse) * 512 ))" -lt 1048576 ]; then sparse_source=yes; else sparse_source=no; fi
$B $SAVE_FLAGS -C /data/sp13 -- work >/data/sp13.tar 2>/tmp/e1; rc=$?
eq "save of a tree with a socket: exit" "$rc" 0
has "save of a tree with a socket: the message" "$(cat /tmp/e1)" "work/sock: "
has "save of a tree with a socket: the reason" "$(cat /tmp/e1)" "pax format cannot archive sockets"
eq "save of a tree with a socket: one line on stderr" "$(grep -c . /tmp/e1)" 1
eq "the socket is not in the tar; the fifo, the device node, the regular and the sparse file are" "$($B -t -f /data/sp13.tar | LC_ALL=C sort | tr "\n" " ")" "work/ work/dev work/fifo work/plain.txt work/sparse "
if [ "$sparse_source" = yes ]; then
  if [ "$(stat -c %s /data/sp13.tar)" -lt 1048576 ]; then pass "a 10 MiB sparse file with one data byte is stored with its holes: the tar is under 1 MiB"; else fail "the tar of the sparse file is $(stat -c %s /data/sp13.tar) bytes"; fi
else
  skip "the file system of the volume does not keep holes: the sparse file is not judged"
fi
$B $RESTORE_FLAGS -C /data/sp13r </data/sp13.tar 2>/tmp/e2; rc=$?
eq "restore as root with the 7 capabilities: exit 1 for the device node" "$rc" 1
has "restore: the message names the device node" "$(cat /tmp/e2)" "work/dev"
has "restore: the device node is refused for lack of permission" "$(cat /tmp/e2)" "Can'\''t create '\''work/dev'\'': Operation not permitted"
eq "restore: the other entries are written" "$(find /data/sp13r -mindepth 1 | LC_ALL=C sort | tr "\n" " ")" "/data/sp13r/work /data/sp13r/work/fifo /data/sp13r/work/plain.txt /data/sp13r/work/sparse "
eq "restore: the fifo is a fifo" "$([ -p /data/sp13r/work/fifo ] && echo yes)" yes
eq "restore: the regular file" "$(cat /data/sp13r/work/plain.txt)" plain
eq "restore: the sparse file is 10 MiB long" "$(stat -c %s /data/sp13r/work/sparse)" 10485760
eq "restore: the last byte of the sparse file" "$(tail -c 1 /data/sp13r/work/sparse)" x
if [ "$sparse_source" = yes ]; then
  if [ "$(( $(stat -c %b /data/sp13r/work/sparse) * 512 ))" -lt 1048576 ]; then pass "restore: the sparse file is sparse again"; else fail "restore: the sparse file holds $(( $(stat -c %b /data/sp13r/work/sparse) * 512 )) bytes"; fi
fi
'
  probe_checks "$setup" -v "${DATA_VOL}:/data" $SEVEN_CAPS --cap-add MKNOD
  probe_checks "$checks" -v "${DATA_VOL}:/data" $SEVEN_CAPS
}

test_13_special_files_and_sparse_files() {
  run_host_test 13 "save keeps holes of a sparse file, skips a socket (message, exit 0), stores a fifo and a device node; restore of a device node needs MKNOD" test_13_body
}
