#!/bin/sh
# Run by scripts/stage-native-clutches.sh from the clutch root: the shim links
# the vendored raylib, so build it first.
set -eu
sh native/vendor/build-raylib.sh "$1" "native/$1/libraylib.a"
