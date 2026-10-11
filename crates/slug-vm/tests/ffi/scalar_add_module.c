#include "slug_ffi_prototype.h"

/*
 * Private benchmark fixture for the 0.13 host-table call path.  Keep this
 * module deliberately small: the timed Slug loop must measure one scalar
 * callback, not I/O or a helper-library operation.
 */
static int32_t add(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  int64_t left;
  int64_t right;

  (void)state;
  if (!host->argument_i64(call, 0, &left) || !host->argument_i64(call, 1, &right)) {
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, left + right);
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
    {sizeof(slug_ffi_function_descriptor), {"add", 3}, {"scalar.add/v1", 13}, 2, 2, add},
};

static const slug_ffi_module_descriptor MODULE = {
    SLUG_FFI_PROTOTYPE_ABI_MAJOR,
    SLUG_FFI_PROTOTYPE_ABI_MINOR,
    sizeof(slug_ffi_module_descriptor),
    {"slug.scalar", 11},
    FUNCTIONS,
    1,
    NULL,
    0,
};

static const slug_ffi_library_descriptor LIBRARY = {
    SLUG_FFI_PROTOTYPE_ABI_MAJOR,
    SLUG_FFI_PROTOTYPE_ABI_MINOR,
    sizeof(slug_ffi_library_descriptor),
    NULL,
    &MODULE,
    1,
};

const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host,
                                                          void **library_state) {
  if (host == NULL || library_state == NULL) return NULL;
  *library_state = NULL;
  return &LIBRARY;
}
