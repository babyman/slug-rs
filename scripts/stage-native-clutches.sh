#!/bin/sh
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

case "$platform" in
  macos-*)
    suffix=dylib
    linker=-dynamiclib
    ;;
  linux-*)
    suffix=so
    linker='-shared -fPIC'
    ;;
  windows-*)
    suffix=dll
    linker=-shared
    ;;
esac

stage() {
  clutch=$1
  source=$2
  output_name=$3
  libraries=${4-}
  output="clutch/$clutch/native/$platform/$output_name.$suffix"
  mkdir -p "$(dirname "$output")"
  # shellcheck disable=SC2086
  cc -I include $linker "$source" -o "$output" $libraries
  echo "staged $output"
}

# Typed clutches: generate the shim from clutch.toml and build the library it names.
stage_typed() {
  clutch=$1
  provider=$2
  cargo run --quiet -p slug-native-manifest --bin slug-native-shim -- \
    stage "clutch/$clutch/clutch.toml" "$provider" "$platform"
}

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

if selected slug.io.fs.clutch "$@"; then
  stage_typed slug.io.fs.clutch slug_io_fs
fi
if selected slug.db.sqlite.clutch "$@"; then
  stage_typed slug.db.sqlite.clutch slug_db_sqlite
fi
if selected slug.core.clutch "$@"; then
  stage slug.core.clutch clutch/slug.core.clutch/native/source/core.c libslug_core -lm
fi
