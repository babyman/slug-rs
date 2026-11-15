#!/bin/sh
# Builds the native library of each clutch with a `[native]` table in a clutch
# repository, for the current platform:
#   stage-native-clutches.sh [--repository DIR] [name.clutch...]
# The repository defaults to this checkout's `clutch/`. With clutch names,
# only those are staged. A clutch that needs something built first, such as
# a vendored static library, provides `native/stage-prerequisites.sh`, which
# runs from the clutch root with the platform as its argument.
set -eu

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) platform=macos-aarch64 ;;
  Darwin-x86_64) platform=macos-x86_64 ;;
  Linux-x86_64) platform=linux-x86_64 ;;
  MINGW*_NT-*-x86_64|MSYS_NT-*-x86_64|CYGWIN_NT-*-x86_64) platform=windows-x86_64 ;;
  *)
    echo "native clutch staging is unsupported on $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

checkout=$(cd "$(dirname "$0")/.." && pwd)
repository="$checkout/clutch"
if [ "${1:-}" = "--repository" ]; then
  [ "$#" -ge 2 ] || { echo "--repository needs a directory" >&2; exit 1; }
  repository=$(cd "$2" && pwd)
  shift 2
fi

# With arguments, stage only the named clutches; otherwise stage them all.
selected() {
  wanted=$1
  shift
  [ "$#" -eq 0 ] && return 0
  for clutch in "$@"; do
    [ "$clutch" = "$wanted" ] && return 0
  done
  return 1
}

for name in "$@"; do
  if [ ! -f "$repository/$name/clutch.toml" ]; then
    echo "no clutch $name in $repository" >&2
    exit 1
  fi
done

staged=0
for root in "$repository"/*.clutch; do
  [ -f "$root/clutch.toml" ] || continue
  name=$(basename "$root")
  selected "$name" "$@" || continue
  grep -q '^\[native\]' "$root/clutch.toml" || continue
  if [ -f "$root/native/stage-prerequisites.sh" ]; then
    (cd "$root" && sh native/stage-prerequisites.sh "$platform")
  fi
  # The provider prefixes the shim's symbols: `slug.io.fs` is `slug_io_fs`.
  provider=$(basename "$name" .clutch | tr . _)
  cargo run --quiet --manifest-path "$checkout/Cargo.toml" \
    -p slug-native-manifest --bin slug-native-shim -- \
    stage "$root/clutch.toml" "$provider" "$platform"
  staged=$((staged + 1))
done
if [ "$#" -gt 0 ] && [ "$staged" -eq 0 ]; then
  echo "none of $* has a [native] table" >&2
  exit 1
fi
