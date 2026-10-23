#!/bin/sh
# Builds the vendored raylib as a static library for one platform:
#   build-raylib.sh <platform> <output.a>
# Only the modules slug.arcade uses are compiled; see VENDOR.md.
set -eu

platform=$1
output=$2
here=$(cd "$(dirname "$0")" && pwd)
src="$here/raylib/src"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cc=${CC:-cc}
flags="-O2 -fPIC -w -DPLATFORM_DESKTOP_GLFW -DGRAPHICS_API_OPENGL_33 -I$src -I$src/external/glfw/include"
case "$platform" in
  macos-*) glfw_flags="-x objective-c" ;;
  linux-*) flags="$flags -D_GNU_SOURCE -D_GLFW_X11"; glfw_flags="" ;;
  *)
    echo "build-raylib.sh: unsupported platform $platform" >&2
    exit 1
    ;;
esac

for unit in rcore rshapes rtextures rtext utils raudio; do
  # shellcheck disable=SC2086
  "$cc" $flags -c "$src/$unit.c" -o "$work/$unit.o"
done
# shellcheck disable=SC2086
"$cc" $flags $glfw_flags -c "$src/rglfw.c" -o "$work/rglfw.o"

mkdir -p "$(dirname "$output")"
rm -f "$output"
ar rcs "$output" "$work"/*.o
