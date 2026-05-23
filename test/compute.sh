#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

./target/debug/cobol2wei test/compute.cbl -o /tmp/compute.wei
./target/debug/wei /tmp/compute.wei -o /tmp/compute

out=$(/tmp/compute)
if [ "$out" = "24420120" ]; then
    echo "compute: PASS"
else
    echo "compute: FAIL -- expected 24420120, got '$out'"
    exit 1
fi
