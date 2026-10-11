#!/bin/sh
# Private spike generator. It intentionally understands only the shared
# fixture table; it is not a manifest format or a public native-code tool.
set -eu

mode=${1:?usage: generate-typed-dispatch-shim.sh normal|mismatch output.c}
output=${2:?usage: generate-typed-dispatch-shim.sh normal|mismatch output.c}

case "$mode" in
  normal)
    cat >"$output" <<'EOF'
#include "typed_dispatch_fixture.h"

/* Generated from the private typed-dispatch fixture table. */
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_handle *slug_shim_open_read(const char *path) { return slug_spike_open_read(path); }
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_handle *slug_shim_open_write(const char *path) { return slug_spike_open_write(path); }
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_handle *slug_shim_open_append(const char *path) { return slug_spike_open_append(path); }
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_status slug_shim_read_line(slug_spike_handle *handle, const char **text) { return slug_spike_read_line(handle, text); }
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT int64_t slug_shim_write(slug_spike_handle *handle, const char *text, uint64_t length) { return slug_spike_write(handle, text, length); }
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_status slug_shim_close(slug_spike_handle *handle) { return slug_spike_close(handle); }
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT int64_t slug_shim_add(int64_t left, int64_t right) { return slug_spike_add(left, right); }
EOF
    ;;
  mismatch)
    cat >"$output" <<'EOF'
#include "typed_dispatch_fixture.h"

/* Deliberately incompatible generated declaration: must fail with -Werror. */
static int64_t (*const slug_shim_add)(int64_t, const char *) = slug_spike_add;
EOF
    ;;
  *)
    echo "unknown mode: $mode" >&2
    exit 64
    ;;
esac
