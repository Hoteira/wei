#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/grade.cbl -o /tmp/grade.wei
./target/debug/wei /tmp/grade.wei -o /tmp/grade

out=$(/tmp/grade)
if [ "$out" = "3" ]; then
    echo "grade: PASS"
else
    echo "grade: FAIL -- expected 3, got '$out'"
    exit 1
fi
