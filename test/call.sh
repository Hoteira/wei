#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/call.cbl -o /tmp/call.wei
./target/debug/wei /tmp/call.wei -o /tmp/call

out=$(/tmp/call)
if [ "$out" = "10" ]; then
    echo "call: PASS"
else
    echo "call: FAIL -- expected 10, got '$out'"
    exit 1
fi
