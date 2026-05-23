#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

printf 'Alice               \xd2\x86\x01\x00\x00\x00\x00\x00Bob                 \x40\x0d\x03\x00\x00\x00\x00\x00Carol               \xa2\x93\x04\x00\x00\x00\x00\x00' > /tmp/employees3.dat

./target/debug/cobol2wei test/lookup.cbl -o /tmp/lookup.wei
./target/debug/wei /tmp/lookup.wei -o /tmp/lookup_bin

out=$(/tmp/lookup_bin)
if [ "$out" = "2000.00" ]; then
    echo "lookup: PASS"
else
    echo "lookup: FAIL -- expected 2000.00, got '$out'"
    exit 1
fi
