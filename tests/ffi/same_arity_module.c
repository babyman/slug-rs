#include "slug_ffi_prototype.h"

static int32_t first(const slug_ffi_host_api *host, slug_ffi_call *call) {
  (void)call;
  host->set_i64(call, 1);
  return SLUG_FFI_OK;
}

static int32_t second(const slug_ffi_host_api *host, slug_ffi_call *call) {
  (void)call;
  host->set_i64(call, 2);
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"first", 5}, {"same.first/v1", 13}, 0, 0, first},
  {sizeof(slug_ffi_function_descriptor), {"second", 6}, {"same.second/v1", 14}, 0, 0, second},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.same", 9},
  FUNCTIONS,
  2,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host) {
  return host == NULL ? NULL : &MODULE;
}
