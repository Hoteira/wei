#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/ifclass.cbl -o /tmp/ifclass.wei
./target/debug/wei /tmp/ifclass.wei -o /tmp/ifclass

out=$(/tmp/ifclass)
if [ "$out" = "3" ]; then
    echo "ifclass: PASS"
else
    echo "ifclass: FAIL -- expected 3, got '$out'"
    exit 1
fi
