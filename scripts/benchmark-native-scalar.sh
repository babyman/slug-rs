#!/bin/sh
# Capture reproducible local evidence for scalar native calls: the VM controls
# and the production typed path.
# The result file is intentionally ignored: timings are host-specific evidence,
# never a CI threshold.
set -eu

result_dir=${SLUG_BENCHMARK_RESULTS_DIR:-docs/planning/local}
result_file=${result_dir}/native-scalar.txt
mkdir -p "$result_dir"

{
  echo "command: cargo test --release -p slug --test typed_native -- --ignored --nocapture --test-threads=1"
  echo "rustc:"
  rustc -Vv
  echo "C compiler:"
  if command -v cc >/dev/null 2>&1; then
    cc --version
  elif command -v clang >/dev/null 2>&1; then
    clang --version
  else
    echo "unavailable"
  fi
  echo "system:"
  uname -a
  if [ "$(uname -s)" = Darwin ]; then
    sysctl -n machdep.cpu.brand_string 2>/dev/null || true
  elif [ -r /proc/cpuinfo ]; then
    sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo | sed -n '1p'
  fi
  cargo test --release -p slug --test typed_native -- --ignored --nocapture --test-threads=1
} >"$result_file" 2>&1

cat "$result_file"
echo "saved raw result: $result_file"
