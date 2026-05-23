#!/bin/sh
set -e
cd "$(dirname "$0")/.."
cargo build --quiet

printf 'Alice               \xd2\x86\x01\x00\x00\x00\x00\x00Bob                 \x40\x0d\x03\x00\x00\x00\x00\x00' > /tmp/employees.dat
rm -f /tmp/report.dat

./target/debug/cobol2wei test/reporter.cbl -o /tmp/reporter.wei
./target/debug/wei /tmp/reporter.wei -o /tmp/reporter_bin

out=$(/tmp/reporter_bin)
if [ "$out" != "3000.50" ]; then
    echo "reporter: FAIL -- total expected 3000.50, got '$out'"
    exit 1
fi
if ! cmp -s /tmp/employees.dat /tmp/report.dat; then
    echo "reporter: FAIL -- report.dat doesn't match employees.dat"
    exit 1
fi

# Money formatting wei-side test.
out2=$(/tmp/wei_money 2>/dev/null) || true
./target/debug/wei test/money.wei -o /tmp/wei_money
out2=$(/tmp/wei_money)
if [ "$out2" != "1,234.56|1,234,567.89|1,234,567,890,123.45" ]; then
    echo "reporter: FAIL -- money got '$out2'"
    exit 1
fi

echo "reporter: PASS"
