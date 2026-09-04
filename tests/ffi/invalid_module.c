#include "slug_ffi_prototype.h"

static const slug_ffi_module_descriptor MODULE = {
  99,
  0,
  sizeof(slug_ffi_module_descriptor),
  "slug.invalid",
  NULL,
  0,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host) {
  (void)host;
  return &MODULE;
}
