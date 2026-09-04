#include "slug_ffi_prototype.h"
#include <limits.h>
#include <math.h>

static slug_ffi_status add(const slug_ffi_host_api *host, slug_ffi_call *call) {
  int64_t left;
  int64_t right;
  if (!host->argument_i64(call, 0, &left) || !host->argument_i64(call, 1, &right)) {
    return SLUG_FFI_ERROR;
  }
  if ((right > 0 && left > INT64_MAX - right) ||
      (right < 0 && left < INT64_MIN - right)) {
    host->set_error(call, "math.range", "integer addition overflowed");
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, left + right);
  return SLUG_FFI_OK;
}

static slug_ffi_status square_root(const slug_ffi_host_api *host, slug_ffi_call *call) {
  double value;
  if (!host->argument_f64(call, 0, &value)) {
    return SLUG_FFI_ERROR;
  }
  if (value < 0.0) {
    host->set_error(call, "math.domain", "sqrt requires a non-negative number");
    return SLUG_FFI_ERROR;
  }
  host->set_f64(call, sqrt(value));
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {"add", 2, 2, add},
  {"sqrt", 1, 1, square_root},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  "slug.math",
  FUNCTIONS,
  2,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host) {
  if (host == NULL || host->abi_major != SLUG_FFI_PROTOTYPE_ABI_MAJOR ||
      host->table_size < sizeof(slug_ffi_host_api)) {
    return NULL;
  }
  return &MODULE;
}
