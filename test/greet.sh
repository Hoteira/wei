#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/greet.cbl -o /tmp/greet.wei
./target/debug/wei /tmp/greet.wei -o /tmp/greet

out=$(/tmp/greet)
expected="Hello, Alice!                 red     green   blue    "
if [ "$out" = "$expected" ]; then
    echo "greet: PASS"
else
    echo "greet: FAIL"
    echo "  expected: '$expected'"
    echo "  got:      '$out'"
    exit 1
fi
