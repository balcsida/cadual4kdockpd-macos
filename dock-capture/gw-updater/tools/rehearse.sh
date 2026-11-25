#!/bin/sh
# Offline rehearsal of tiwrite against its built-in chip model: no device access.
# Usage (from gw-updater/): tools/rehearse.sh <tiwrite binary>     exits 0 if every scenario ends as expected.
# Exit code 3 from tiwrite means its verdict disagrees with the modelled flash: always a failure here.
T=$1; P=itec-ti-flash-patched-trysrc0.bin; O=itec-ti-flash-original.bin; fail=0
t() {
    want=$1; name=$2; shift 2
    out=$("$@" 2>&1); rc=$?; last=$(printf '%s\n' "$out" | tail -1)
    if [ $rc -eq "$want" ]; then echo "ok   $name: $last"; else echo "FAIL $name (exit $rc, wanted $want): $last"; fail=1; fi
}
t 0 "normal run"                "$T" simulate $P --expect-current 7943 --flash $O
t 0 "half-filled chip buffer"   "$T" simulate $P --expect-current 7943 --flash $O stale=236
t 0 "acknowledgement 700 lost"  "$T" simulate $P --expect-current 7943 --flash $O dropack=700
t 0 "last acknowledgement lost" "$T" simulate $P --expect-current 7943 --flash $O dropack=3295
t 0 "first erase fails"         "$T" simulate $P --expect-current 7943 --flash $O failerase=1
t 0 "first page skipped"        "$T" simulate $P --expect-current 7943 --flash $O skippage=0
t 0 "page 300 skipped"          "$T" simulate $P --expect-current 7943 --flash $O skippage=300
t 0 "last page skipped"         "$T" simulate $P --expect-current 7943 --flash $O skippage=658
t 0 "wrong byte programmed"     "$T" simulate $P --expect-current 7943 --flash $O badbyte=10496
t 1 "content is not the backup" "$T" simulate $P --expect-current 1234 --flash $O
t 1 "every erase fails"         "$T" simulate $P --expect-current 7943 --flash $O failerase=9
t 0 "way back: original image"  "$T" simulate $O --expect-current 78C9 --flash $P
exit $fail
