#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

printf 'Alice               \xd2\x86\x01\x00\x00\x00\x00\x00Bob                 \x40\x0d\x03\x00\x00\x00\x00\x00' > /tmp/raise.dat

./target/debug/cobol2wei test/raise.cbl -o /tmp/raise.wei
./target/debug/wei /tmp/raise.wei -o /tmp/raise_bin

out=$(/tmp/raise_bin)
if [ "$out" = "3200.50" ]; then
    echo "raise: PASS"
else
    echo "raise: FAIL -- expected 3200.50, got '$out'"
    exit 1
fi
