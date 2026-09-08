#include "slug_ffi_prototype.h"
#include <stdlib.h>

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})

static void destroy_resource(void *resource) { free(resource); }

static int32_t create_alpha(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  int64_t *resource = malloc(sizeof(int64_t));
  if (resource == NULL) return SLUG_FFI_ERROR;
  *resource = 1;
  if (!host->set_resource(call, TEXT("slug.alpha.Handle"), resource)) { free(resource); return SLUG_FFI_ERROR; }
  return SLUG_FFI_OK;
}

static int32_t read_alpha(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *resource = NULL;
  if (!host->argument_resource(call, 0, TEXT("slug.alpha.Handle"), &resource)) return SLUG_FFI_ERROR;
  host->set_i64(call, *(int64_t *)resource);
  return SLUG_FFI_OK;
}

static int32_t create_beta(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  int64_t *resource = malloc(sizeof(int64_t));
  if (resource == NULL) return SLUG_FFI_ERROR;
  *resource = 2;
  if (!host->set_resource(call, TEXT("slug.beta.Handle"), resource)) { free(resource); return SLUG_FFI_ERROR; }
  return SLUG_FFI_OK;
}

static int32_t read_beta(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *resource = NULL;
  if (!host->argument_resource(call, 0, TEXT("slug.beta.Handle"), &resource)) return SLUG_FFI_ERROR;
  host->set_i64(call, *(int64_t *)resource);
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor ALPHA_FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("create"), TEXT("alpha.create/v1"), 0, 0, create_alpha},
  {sizeof(slug_ffi_function_descriptor), TEXT("read"), TEXT("alpha.read/v1"), 1, 1, read_alpha},
};
static const slug_ffi_function_descriptor BETA_FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("create"), TEXT("beta.create/v1"), 0, 0, create_beta},
  {sizeof(slug_ffi_function_descriptor), TEXT("read"), TEXT("beta.read/v1"), 1, 1, read_beta},
};
static const slug_ffi_resource_descriptor RESOURCES[] = {
  {sizeof(slug_ffi_resource_descriptor), TEXT("Handle"), destroy_resource},
};
static const slug_ffi_module_descriptor MODULES[] = {
  {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_module_descriptor), TEXT("slug.alpha"), ALPHA_FUNCTIONS, 2, RESOURCES, 1},
  {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_module_descriptor), TEXT("slug.beta"), BETA_FUNCTIONS, 2, RESOURCES, 1},
};
static const slug_ffi_library_descriptor LIBRARY = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_library_descriptor), NULL, MODULES, 2,
};

const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host,
                                                          void **library_state) {
  if (host == NULL || library_state == NULL) return NULL;
  *library_state = NULL;
  return &LIBRARY;
}
