#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/birthday.cbl -o /tmp/birthday.wei
./target/debug/wei /tmp/birthday.wei -o /tmp/birthday

out=$(/tmp/birthday)
if [ "$out" = "20260523" ]; then
    echo "birthday: PASS"
else
    echo "birthday: FAIL -- expected 20260523, got '$out'"
    exit 1
fi
