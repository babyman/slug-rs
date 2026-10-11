#ifndef SLUG_TYPED_SHIM_ABI_H
#define SLUG_TYPED_SHIM_ABI_H

#include <stdint.h>

#define SLUG_SHIM_ABI_VERSION 1u

typedef struct {
  union { int64_t i64; uint64_t u64; double f64; const void *pointer; } as;
  uint64_t length;
} slug_value;

typedef struct { void (*release)(void *); void *context; } slug_shim_cleanup;
typedef int32_t (*slug_shim_entry)(const slug_value *, slug_value *, slug_shim_cleanup *);
typedef struct {
  const char *module;
  const char *name;
  slug_shim_entry entry;
  uint32_t argument_count;
  uint32_t result_count;
  uint64_t signature_hash;
} slug_shim_binding;
typedef struct {
  uint32_t abi_version;
  uint32_t binding_count;
  const slug_shim_binding *bindings;
  uint64_t table_hash;
} slug_shim_table;

#endif
