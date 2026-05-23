#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/count.cbl -o /tmp/count.wei
./target/debug/wei /tmp/count.wei -o /tmp/count_bin

out=$(/tmp/count_bin)
if [ "$out" = "44" ]; then
    echo "count: PASS"
else
    echo "count: FAIL -- expected 44, got '$out'"
    exit 1
fi
