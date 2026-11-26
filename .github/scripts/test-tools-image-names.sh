#!/usr/bin/env bash

test_6_names() {
  local script='
B=/opt/sbx/bin/bsdtar
mkdir -p /data/src6 && cd /data/src6
touch "$(printf "raw\377\376name")" "$(printf "caf\303\251")" "$(printf "\346\227\245\346\234\254\350\252\236")" "$(printf "emoji\360\237\230\200")" "$(printf "latin1\351")"
mkdir -p "$(printf "d\377ir")"; printf data > "$(printf "d\377ir/f\377")"
names() { (cd "$1" && find . -print0 | LC_ALL=C sort -z | sha256sum | cut -c1-16); }
want=$(names /data/src6)
CREATE_FLAGS="--create --file /data/x.tar --format pax --numeric-owner"
trial() {
  label="$1"; create_opt="$2"; extract_opt="$3"; shift 3
  rm -rf /data/dst6; mkdir -p /data/dst6
  env "$@" $B $CREATE_FLAGS $create_opt -C /data/src6 -- . 2>/data/ce; create_rc=$?
  env "$@" $B --extract --file /data/x.tar --numeric-owner $extract_opt -C /data/dst6 2>/data/ee; extract_rc=$?
  if [ "$(names /data/dst6)" = "$want" ]; then names_state=equal; else names_state=different; fi
  create_err=$(cat /data/ce); extract_err=$(cat /data/ee)
  info "$label: create=$create_rc extract=$extract_rc names=$names_state"
}

trial "default options, locale C" "" "" LC_ALL=C
eq "default, C: create exit" "$create_rc" 0
eq "default, C: extract exit" "$extract_rc" 0
eq "default, C: names" "$names_state" equal
has "default, C: warning on create" "$create_err" "Can'"'"'t translate pathname"
has "default, C: warning names the target charset" "$create_err" "to UTF-8"

trial "default options, locale C.UTF-8" "" "" LC_ALL=C.UTF-8
eq "default, C.UTF-8: create exit" "$create_rc" 0
eq "default, C.UTF-8: extract exit" "$extract_rc" 0
eq "default, C.UTF-8: names" "$names_state" equal
has "default, C.UTF-8: warning on create" "$create_err" "Can'"'"'t translate pathname"

for locale in C C.UTF-8; do
  trial "hdrcharset=BINARY on create, locale $locale" "--options hdrcharset=BINARY" "" LC_ALL=$locale
  eq "BINARY on create, $locale: create exit" "$create_rc" 0
  eq "BINARY on create, $locale: extract exit" "$extract_rc" 0
  eq "BINARY on create, $locale: names" "$names_state" equal
  eq "BINARY on create, $locale: create stderr" "$create_err" ""
  eq "BINARY on create, $locale: extract stderr" "$extract_err" ""
done

trial "hdrcharset=BINARY on create and extract, locale C" "--options hdrcharset=BINARY" "--options hdrcharset=BINARY" LC_ALL=C
eq "BINARY on extract: create exit" "$create_rc" 0
eq "BINARY on extract: extract exit" "$extract_rc" 1
has "BINARY on extract: message" "$extract_err" "iconv_open failed: Cannot handle"
eq "BINARY on extract: names are not restored" "$names_state" different
'
  run_scripted_test 6 "raw-byte and Unicode names survive create then extract in the static musl build; hdrcharset=BINARY on create only" "$script" $SEVEN_CAPS
}

test_8_special_entries() {
  local script='
B=/opt/sbx/bin/bsdtar
mkdir -p /data/src8 /data/dst8 && cd /data/src8
printf one > "a b"
printf two > "$(printf "new\nline")"
printf three > "tab	name"
: > zero_byte
ln "a b" hard_one; ln "a b" hard_two
mkdir "dir with space"; printf x > "dir with space/in side"
names() { (cd "$1" && find . -printf "%p\t%y\t%s\t%n\t%m\0" | LC_ALL=C sort -z | tr "\0" "\n" | sed "s/\t/|/g" | tr "\n" ";"); }
before=$(names /data/src8)
$B $SAVE_FLAGS -C /data/src8 -- . 2>/data/ce8 | $B $RESTORE_FLAGS -C /data/dst8 2>/data/ee8
exit_codes=("${PIPESTATUS[@]}")
eq "create exit" "${exit_codes[0]}" 0
eq "restore exit" "${exit_codes[1]}" 0
eq "create stderr" "$(cat /data/ce8)" ""
eq "extract stderr" "$(cat /data/ee8)" ""
eq "listing (names, types, sizes, link counts, modes)" "$(names /data/dst8)" "$before"
if [ /data/dst8/hard_one -ef /data/dst8/hard_two ] && [ "/data/dst8/a b" -ef /data/dst8/hard_one ]; then pass "hard links share one inode after restore"; else fail "hard links lost"; fi
if [ -f /data/dst8/zero_byte ] && [ ! -s /data/dst8/zero_byte ]; then pass "0-byte file kept"; else fail "0-byte file"; fi
if [ -f "/data/dst8/$(printf "new\nline")" ]; then pass "name with a newline kept"; else fail "newline name"; fi
if [ -f "/data/dst8/dir with space/in side" ]; then pass "names with spaces kept"; else fail "space name"; fi
'
  run_scripted_test 8 "names with space, tab and newline; hard links; 0-byte file; README save and restore lines" "$script" $SEVEN_CAPS
}
