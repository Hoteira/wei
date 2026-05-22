#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

printf 'Alice               \xd2\x86\x01\x00\x00\x00\x00\x00Bob                 \x40\x0d\x03\x00\x00\x00\x00\x00' > /tmp/employees.dat

./target/debug/cobol2wei test/empsums.cbl -o /tmp/empsums.wei
./target/debug/wei /tmp/empsums.wei -o /tmp/empsums

out=$(/tmp/empsums)
if [ "$out" = "3000.50" ]; then
    echo "empsums: PASS"
else
    echo "empsums: FAIL -- expected 3000.50, got '$out'"
    exit 1
fi
