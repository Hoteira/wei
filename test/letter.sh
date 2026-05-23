#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/letter.cbl -o /tmp/letter.wei
./target/debug/wei /tmp/letter.wei -o /tmp/letter

out=$(/tmp/letter)
if [ "$out" = "123" ]; then
    echo "letter: PASS"
else
    echo "letter: FAIL -- expected 123, got '$out'"
    exit 1
fi
