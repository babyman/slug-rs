#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
target_dir=${CARGO_TARGET_DIR:-"$root_dir/target"}
case "$target_dir" in
    /*) ;;
    *) target_dir="$root_dir/$target_dir" ;;
esac
binary="$target_dir/release/slug"
output_file=$(mktemp "${TMPDIR:-/tmp}/slug-vm-memory.XXXXXX")
trap 'rm -f "$output_file"' EXIT HUP INT TERM

(cd "$root_dir" && cargo build --release --bin slug)

case "$(uname -s)" in
    Darwin)
        measure() {
            /usr/bin/time -l "$binary" "$1" >/dev/null 2>"$output_file"
            awk '/maximum resident set size$/ { print $1; exit }' "$output_file"
        }
        ;;
    Linux)
        measure() {
            /usr/bin/time -f '%M' "$binary" "$1" >/dev/null 2>"$output_file"
            awk 'END { printf "%.0f\n", $1 * 1024 }' "$output_file"
        }
        ;;
    *)
        printf '%s\n' "unsupported platform for peak-RSS measurement: $(uname -s)" >&2
        exit 1
        ;;
esac

printf '%-30s %15s\n' 'workload' 'peak RSS (bytes)'
for fixture in \
    crates/slug-vm/benches/memory/minimal.slug \
    crates/slug-vm/benches/memory/closures-retained-128.slug \
    crates/slug-vm/benches/memory/closures-retained-1024.slug \
    crates/slug-vm/benches/memory/maps-retained-128.slug \
    crates/slug-vm/benches/memory/maps-retained-1024.slug
do
    rss=$(measure "$root_dir/$fixture")
    if [ -z "$rss" ]; then
        printf '%s\n' "could not read peak RSS for $fixture" >&2
        exit 1
    fi
    printf '%-30s %15s\n' "${fixture#crates/slug-vm/benches/memory/}" "$rss"
done
