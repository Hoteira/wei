#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/jumper.cbl -o /tmp/jumper.wei
./target/debug/wei /tmp/jumper.wei -o /tmp/jumper

out=$(/tmp/jumper)
if [ "$out" = "123" ]; then
    echo "jumper: PASS"
else
    echo "jumper: FAIL -- expected 123, got '$out'"
    exit 1
fi
