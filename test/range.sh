#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/range.cbl -o /tmp/range.wei
./target/debug/wei /tmp/range.wei -o /tmp/range

out=$(/tmp/range)
if [ "$out" = "123" ]; then
    echo "range: PASS"
else
    echo "range: FAIL -- expected 123, got '$out'"
    exit 1
fi
