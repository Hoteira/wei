#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/wei test/subrec.wei -o /tmp/subrec

out=$(/tmp/subrec)
expected="9|1|HeLLo     "
if [ "$out" = "$expected" ]; then
    echo "subrec: PASS"
else
    echo "subrec: FAIL -- expected '$expected', got '$out'"
    exit 1
fi
