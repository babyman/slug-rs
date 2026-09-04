#include "slug_ffi_prototype.h"
#include <stdlib.h>

typedef struct {
  int serial;
} module_state;

static int next_serial = 1;
static int destroyed = 0;

static int32_t state_info(const slug_ffi_host_api *host, slug_ffi_call *call, void *raw_state) {
  module_state *state = raw_state;
  if (state == NULL) {
    host->set_error(call, (slug_ffi_text){"state.missing", 13},
                    (slug_ffi_text){"module state is unavailable", 27});
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, state->serial * 100 + destroyed);
  return SLUG_FFI_OK;
}

static void destroy_module(void *raw_state) {
  destroyed += 1;
  free(raw_state);
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"stateInfo", 9}, {"state.info/v1", 13}, 0, 0, state_info},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.stateful", 13},
  destroy_module,
  FUNCTIONS,
  1,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  module_state *state = malloc(sizeof(module_state));
  if (state == NULL) return NULL;
  state->serial = next_serial++;
  *out_state = state;
  return &MODULE;
}
