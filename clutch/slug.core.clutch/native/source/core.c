#include "slug_ffi_prototype.h"

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})

static int32_t keys(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  slug_ffi_value map, key, ignored;
  uint64_t length, index;
  slug_ffi_list *result;
  if (!host->argument_value(call, 0, &map) || !host->value_map_length(call, map, &length)) return SLUG_FFI_ERROR;
  result = host->list_create(call, length);
  if (result == NULL) return SLUG_FFI_ERROR;
  for (index = 0; index < length; index++) {
    if (!host->value_map_entry(call, map, index, &key, &ignored) || !host->list_append_value(call, result, key)) {
      host->list_destroy(result);
      return SLUG_FFI_ERROR;
    }
  }
  return host->set_list(call, result) ? SLUG_FFI_OK : SLUG_FFI_ERROR;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("keys"), TEXT("std.keys/v1"), 1, 1, keys},
};
static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_module_descriptor),
  TEXT("slug.std"), FUNCTIONS, 1, NULL, 0,
};
static const slug_ffi_library_descriptor LIBRARY = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_library_descriptor),
  NULL, &MODULE, 1,
};

SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_library_descriptor *slug_ffi_library_init(
    const slug_ffi_host_api *host, void **library_state) {
  if (host == NULL || host->abi_major != SLUG_FFI_PROTOTYPE_ABI_MAJOR ||
      host->table_size < sizeof(slug_ffi_host_api) || library_state == NULL) return NULL;
  *library_state = NULL;
  return &LIBRARY;
}
