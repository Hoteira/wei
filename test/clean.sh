#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/clean.cbl -o /tmp/clean.wei
./target/debug/wei /tmp/clean.wei -o /tmp/clean

out=$(/tmp/clean)
expected="555-123-4567  555 123 4567  "
if [ "$out" = "$expected" ]; then
    echo "clean: PASS"
else
    echo "clean: FAIL -- expected '$expected', got '$out'"
    exit 1
fi
