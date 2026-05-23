#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/wei test/inc_main.wei -o /tmp/inc

out=$(/tmp/inc)
if [ "$out" = "14" ]; then
    echo "inc: PASS"
else
    echo "inc: FAIL -- expected 14, got '$out'"
    exit 1
fi
