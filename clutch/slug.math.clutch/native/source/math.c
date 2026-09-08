#include "slug_ffi_prototype.h"
#include <limits.h>
#include <math.h>

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})

static int32_t add(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  int64_t left, right;
  if (!host->argument_i64(call, 0, &left) || !host->argument_i64(call, 1, &right)) return SLUG_FFI_ERROR;
  if ((right > 0 && left > INT64_MAX - right) || (right < 0 && left < INT64_MIN - right)) {
    host->set_error(call, TEXT("math.range"), TEXT("integer addition overflowed"));
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, left + right);
  return SLUG_FFI_OK;
}

static int32_t square_root(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  double value;
  if (!host->argument_f64(call, 0, &value)) return SLUG_FFI_ERROR;
  if (value < 0.0) { host->set_error(call, TEXT("math.domain"), TEXT("sqrt requires a non-negative number")); return SLUG_FFI_ERROR; }
  host->set_f64(call, sqrt(value));
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("add"), TEXT("math.add/v1"), 2, 2, add},
  {sizeof(slug_ffi_function_descriptor), TEXT("sqrt"), TEXT("math.sqrt/v1"), 1, 1, square_root},
};
static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_module_descriptor),
  TEXT("slug.math"), NULL, FUNCTIONS, 2,
};
static const slug_ffi_library_descriptor LIBRARY = {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_library_descriptor), NULL, &MODULE, 1};
SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host, void **module_state) {
  if (host == NULL || host->abi_major != SLUG_FFI_PROTOTYPE_ABI_MAJOR || host->table_size < sizeof(slug_ffi_host_api) || module_state == NULL) return NULL;
  *module_state = NULL;
  return &LIBRARY;
}
