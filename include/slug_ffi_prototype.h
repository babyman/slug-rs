#ifndef SLUG_FFI_PROTOTYPE_H
#define SLUG_FFI_PROTOTYPE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define SLUG_FFI_PROTOTYPE_ABI_MAJOR 0u
#define SLUG_FFI_PROTOTYPE_ABI_MINOR 1u

typedef enum {
  SLUG_FFI_OK = 0,
  SLUG_FFI_ERROR = 1,
} slug_ffi_status;

typedef struct slug_ffi_host_api slug_ffi_host_api;
typedef struct slug_ffi_call slug_ffi_call;

typedef bool (*slug_ffi_argument_i64_fn)(slug_ffi_call *, size_t, int64_t *);
typedef bool (*slug_ffi_argument_f64_fn)(slug_ffi_call *, size_t, double *);
typedef void (*slug_ffi_set_i64_fn)(slug_ffi_call *, int64_t);
typedef void (*slug_ffi_set_f64_fn)(slug_ffi_call *, double);
typedef void (*slug_ffi_set_error_fn)(slug_ffi_call *, const char *, const char *);

struct slug_ffi_host_api {
  uint32_t abi_major;
  uint32_t abi_minor;
  size_t table_size;
  slug_ffi_argument_i64_fn argument_i64;
  slug_ffi_argument_f64_fn argument_f64;
  slug_ffi_set_i64_fn set_i64;
  slug_ffi_set_f64_fn set_f64;
  slug_ffi_set_error_fn set_error;
};

typedef slug_ffi_status (*slug_ffi_callback)(const slug_ffi_host_api *, slug_ffi_call *);

typedef struct {
  const char *name;
  size_t minimum_arity;
  size_t maximum_arity;
  slug_ffi_callback callback;
} slug_ffi_function_descriptor;

typedef struct {
  uint32_t abi_major;
  uint32_t abi_minor;
  size_t descriptor_size;
  const char *module_name;
  const slug_ffi_function_descriptor *functions;
  size_t function_count;
} slug_ffi_module_descriptor;

typedef const slug_ffi_module_descriptor *(*slug_ffi_module_init_fn)(const slug_ffi_host_api *);

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host);

#endif
