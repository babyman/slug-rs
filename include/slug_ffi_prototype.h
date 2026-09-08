#ifndef SLUG_FFI_PROTOTYPE_H
#define SLUG_FFI_PROTOTYPE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#define SLUG_FFI_PROTOTYPE_EXPORT __declspec(dllexport)
#else
#define SLUG_FFI_PROTOTYPE_EXPORT
#endif

#define SLUG_FFI_PROTOTYPE_ABI_MAJOR 0u
#define SLUG_FFI_PROTOTYPE_ABI_MINOR 9u

typedef enum {
  SLUG_FFI_OK = 0,
  SLUG_FFI_ERROR = 1,
} slug_ffi_status;

typedef enum {
  SLUG_FFI_PRODUCER_SENT = 0,
  SLUG_FFI_PRODUCER_FULL = 1,
  SLUG_FFI_PRODUCER_CLOSED = 2,
  SLUG_FFI_PRODUCER_INVALID = 3,
} slug_ffi_producer_status;

typedef struct slug_ffi_host_api slug_ffi_host_api;
typedef struct slug_ffi_call slug_ffi_call;
typedef struct slug_ffi_channel slug_ffi_channel;
typedef struct slug_ffi_producer slug_ffi_producer;
typedef struct slug_ffi_list slug_ffi_list;
typedef struct slug_ffi_map slug_ffi_map;

typedef struct {
  const char *data;
  uint64_t length;
} slug_ffi_text;

typedef enum {
  SLUG_FFI_VALUE_NIL = 0,
  SLUG_FFI_VALUE_INT = 1,
  SLUG_FFI_VALUE_FLOAT = 2,
  SLUG_FFI_VALUE_TEXT = 3,
  SLUG_FFI_VALUE_BYTES = 4,
} slug_ffi_value_kind;

typedef bool (*slug_ffi_argument_i64_fn)(slug_ffi_call *, size_t, int64_t *);
typedef uint64_t (*slug_ffi_argument_count_fn)(slug_ffi_call *);
typedef bool (*slug_ffi_argument_f64_fn)(slug_ffi_call *, size_t, double *);
typedef bool (*slug_ffi_argument_text_fn)(slug_ffi_call *, size_t, slug_ffi_text *);
typedef bool (*slug_ffi_argument_bytes_fn)(slug_ffi_call *, size_t, slug_ffi_text *);
typedef bool (*slug_ffi_argument_kind_fn)(slug_ffi_call *, size_t, slug_ffi_value_kind *);
typedef bool (*slug_ffi_argument_resource_fn)(slug_ffi_call *, size_t, slug_ffi_text, void **);
typedef void (*slug_ffi_set_i64_fn)(slug_ffi_call *, int64_t);
typedef void (*slug_ffi_set_f64_fn)(slug_ffi_call *, double);
typedef void (*slug_ffi_set_nil_fn)(slug_ffi_call *);
typedef bool (*slug_ffi_set_text_fn)(slug_ffi_call *, slug_ffi_text);
typedef slug_ffi_list *(*slug_ffi_list_create_fn)(slug_ffi_call *, uint64_t);
typedef void (*slug_ffi_list_destroy_fn)(slug_ffi_list *);
typedef slug_ffi_map *(*slug_ffi_map_create_fn)(slug_ffi_call *, uint64_t);
typedef void (*slug_ffi_map_destroy_fn)(slug_ffi_map *);
typedef bool (*slug_ffi_map_set_nil_fn)(slug_ffi_call *, slug_ffi_map *, slug_ffi_text);
typedef bool (*slug_ffi_map_set_i64_fn)(slug_ffi_call *, slug_ffi_map *, slug_ffi_text, int64_t);
typedef bool (*slug_ffi_map_set_f64_fn)(slug_ffi_call *, slug_ffi_map *, slug_ffi_text, double);
typedef bool (*slug_ffi_map_set_text_fn)(slug_ffi_call *, slug_ffi_map *, slug_ffi_text, slug_ffi_text);
typedef bool (*slug_ffi_map_set_bytes_fn)(slug_ffi_call *, slug_ffi_map *, slug_ffi_text, slug_ffi_text);
typedef bool (*slug_ffi_list_append_map_fn)(slug_ffi_call *, slug_ffi_list *, slug_ffi_map *);
typedef bool (*slug_ffi_set_list_fn)(slug_ffi_call *, slug_ffi_list *);
typedef void (*slug_ffi_set_error_fn)(slug_ffi_call *, slug_ffi_text, slug_ffi_text);
typedef bool (*slug_ffi_set_resource_fn)(slug_ffi_call *, slug_ffi_text, void *);
typedef bool (*slug_ffi_close_resource_fn)(slug_ffi_call *, size_t, slug_ffi_text);
typedef slug_ffi_channel *(*slug_ffi_channel_create_fn)(slug_ffi_call *, uint64_t,
                                                         slug_ffi_producer **);
typedef bool (*slug_ffi_set_channel_fn)(slug_ffi_call *, slug_ffi_channel *);
typedef void (*slug_ffi_channel_destroy_fn)(slug_ffi_channel *);
typedef int32_t (*slug_ffi_producer_send_i64_fn)(slug_ffi_producer *, int64_t);
typedef void (*slug_ffi_producer_text_destroy_fn)(void *);
typedef int32_t (*slug_ffi_producer_send_text_fn)(slug_ffi_producer *, slug_ffi_text,
                                                   slug_ffi_producer_text_destroy_fn);
typedef void (*slug_ffi_producer_destroy_fn)(slug_ffi_producer *);

struct slug_ffi_host_api {
  uint32_t abi_major;
  uint32_t abi_minor;
  uint32_t table_size;
  slug_ffi_argument_i64_fn argument_i64;
  slug_ffi_argument_f64_fn argument_f64;
  slug_ffi_argument_text_fn argument_text;
  slug_ffi_argument_resource_fn argument_resource;
  slug_ffi_set_i64_fn set_i64;
  slug_ffi_set_f64_fn set_f64;
  slug_ffi_set_error_fn set_error;
  slug_ffi_set_resource_fn set_resource;
  slug_ffi_close_resource_fn close_resource;
  slug_ffi_channel_create_fn channel_create;
  slug_ffi_set_channel_fn set_channel;
  slug_ffi_channel_destroy_fn channel_destroy;
  slug_ffi_producer_send_i64_fn producer_send_i64;
  slug_ffi_producer_destroy_fn producer_destroy;
  slug_ffi_producer_send_text_fn producer_send_text;
  slug_ffi_set_nil_fn set_nil;
  slug_ffi_set_text_fn set_text;
  slug_ffi_argument_bytes_fn argument_bytes;
  slug_ffi_argument_kind_fn argument_kind;
  slug_ffi_list_create_fn list_create;
  slug_ffi_list_destroy_fn list_destroy;
  slug_ffi_map_create_fn map_create;
  slug_ffi_map_destroy_fn map_destroy;
  slug_ffi_map_set_nil_fn map_set_nil;
  slug_ffi_map_set_i64_fn map_set_i64;
  slug_ffi_map_set_f64_fn map_set_f64;
  slug_ffi_map_set_text_fn map_set_text;
  slug_ffi_map_set_bytes_fn map_set_bytes;
  slug_ffi_list_append_map_fn list_append_map;
  slug_ffi_set_list_fn set_list;
  slug_ffi_argument_count_fn argument_count;
};

typedef int32_t (*slug_ffi_callback)(const slug_ffi_host_api *, slug_ffi_call *, void *);
typedef void (*slug_ffi_module_destroy_fn)(void *);
typedef void (*slug_ffi_resource_destroy_fn)(void *);

typedef struct {
  uint32_t descriptor_size;
  slug_ffi_text name;
  slug_ffi_text member_key;
  uint64_t minimum_arity;
  uint64_t maximum_arity;
  slug_ffi_callback callback;
} slug_ffi_function_descriptor;

typedef struct {
  uint32_t descriptor_size;
  slug_ffi_text name;
  slug_ffi_resource_destroy_fn destroy_resource;
} slug_ffi_resource_descriptor;

typedef struct {
  uint32_t abi_major;
  uint32_t abi_minor;
  uint32_t descriptor_size;
  slug_ffi_text module_name;
  slug_ffi_module_destroy_fn destroy_module;
  const slug_ffi_function_descriptor *functions;
  uint64_t function_count;
  const slug_ffi_resource_descriptor *resources;
  uint64_t resource_count;
} slug_ffi_module_descriptor;

typedef struct {
  uint32_t abi_major;
  uint32_t abi_minor;
  uint32_t descriptor_size;
  slug_ffi_module_destroy_fn destroy_library;
  const slug_ffi_module_descriptor *modules;
  uint64_t module_count;
} slug_ffi_library_descriptor;

typedef const slug_ffi_library_descriptor *(*slug_ffi_library_init_fn)(
    const slug_ffi_host_api *, void **module_state);

SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_library_descriptor *slug_ffi_library_init(
    const slug_ffi_host_api *host, void **module_state);

#endif
