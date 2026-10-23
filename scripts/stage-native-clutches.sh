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

# Generates the shim from clutch.toml and builds the library it names.
stage() {
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
  stage slug.io.fs.clutch slug_io_fs
fi
if selected slug.db.sqlite.clutch "$@"; then
  stage slug.db.sqlite.clutch slug_db_sqlite
fi
if selected slug.net.tcp.clutch "$@"; then
  stage slug.net.tcp.clutch slug_net_tcp
fi
if selected slug.core.clutch "$@"; then
  stage slug.core.clutch slug_core
fi
if selected slug.arcade.clutch "$@"; then
  # The shim links the vendored raylib, so build it first.
  sh clutch/slug.arcade.clutch/native/vendor/build-raylib.sh "$platform" \
    "clutch/slug.arcade.clutch/native/$platform/libraylib.a"
  stage slug.arcade.clutch slug_arcade
fi
