#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/status.cbl -o /tmp/status.wei
./target/debug/wei /tmp/status.wei -o /tmp/status

out=$(/tmp/status)
if [ "$out" = "123" ]; then
    echo "status: PASS"
else
    echo "status: FAIL -- expected 123, got '$out'"
    exit 1
fi
