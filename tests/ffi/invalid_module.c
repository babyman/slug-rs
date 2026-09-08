#include "slug_ffi_prototype.h"

static const slug_ffi_module_descriptor MODULE = {
  99,
  0,
  sizeof(slug_ffi_module_descriptor),
  {"slug.invalid", 12},
  NULL,
  NULL,
  0,
};

static const slug_ffi_library_descriptor LIBRARY = {99, 0, sizeof(slug_ffi_library_descriptor), NULL, &MODULE, 1};
const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host,
                                                        void **module_state) {
  (void)host;
  if (module_state != NULL) *module_state = NULL;
  return &LIBRARY;
}
