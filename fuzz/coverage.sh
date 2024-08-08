#!/usr/bin/bash -e

# cargo install rustfilt
# cargo install cargo-binutils
# apt install aha

cd "$(dirname "$0")" || exit 1
PROJROOT="$(cd .. && pwd)"
cargo fuzz coverage -D diff
rust-cov report --use-color --instr-profile=coverage/diff/coverage.profdata ./target/*/coverage/*/debug/diff -Xdemangler=rustfilt --show-functions --sources ../src/* | aha -b | sed 's#'"${PROJROOT}"/'##g' > coverage/diff/stats.html
rust-cov show --format=html --instr-profile=coverage/diff/coverage.profdata ./target/*/coverage/*/debug/diff -Xdemangler=rustfilt --show-instantiations=false --sources ../src/* | sed 's#'"${PROJROOT}"/'##g' > coverage/diff/files.html
rust-cov show --format=html --instr-profile=coverage/diff/coverage.profdata ./target/*/coverage/*/debug/diff -Xdemangler=rustfilt --show-instantiations=true --sources ../src/* | sed 's#'"${PROJROOT}"/'##g' > coverage/diff/instantiations.html
