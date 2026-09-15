#include "slug_ffi_prototype.h"

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})

static int32_t rejects_stale_map(const slug_ffi_host_api *host, slug_ffi_call *call,
                                 void *state) {
  (void)state;
  slug_ffi_map *map = host->map_create(call, 0);
  if (map == NULL) return SLUG_FFI_ERROR;
  host->map_destroy(map);
  if (host->map_set_i64(call, map, TEXT("value"), 1)) {
    host->set_error(call, TEXT("fixture.failure"), TEXT("stale map was accepted"));
  }
  return SLUG_FFI_ERROR;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("staleMap"), TEXT("handles.stale_map/v1"),
   0, 0, rejects_stale_map},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  TEXT("slug.handles"),
  FUNCTIONS,
  1,
  NULL,
  0,
};

static const slug_ffi_library_descriptor LIBRARY = {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_library_descriptor), NULL, &MODULE, 1};
const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &LIBRARY;
}
