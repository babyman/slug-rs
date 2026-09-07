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

stage slug.io.fs.clutch clutch/slug.io.fs.clutch/native/source/fs.c libslug_io_fs
stage slug.db.sqlite.clutch clutch/slug.db.sqlite.clutch/native/source/sqlite.c libslug_db_sqlite -lsqlite3
stage slug.math.clutch clutch/slug.math.clutch/native/source/math.c libslug_math -lm
