#include "slug_ffi_prototype.h"
#include "slug_ffi_helpers.h"

#include <limits.h>
#include <math.h>
#include <stdio.h>

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})
#define STDIN_CHANNEL_CAPACITY 32u

typedef struct {
  slug_ffi_async_stream *stdin;
} core_state;

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

static void stdin_worker(slug_ffi_async_sender *out, void *context) {
  char *line = NULL;
  size_t length = 0;
  size_t capacity = 0;
  int character;
  (void)context;

  while ((character = fgetc(stdin)) != EOF) {
    if (character == '\n') {
      if (length > 0 && line[length - 1] == '\r') length--;
      if (line == NULL) {
        line = malloc(1);
        if (line == NULL) return;
      }
      if (!slug_ffi_async_sender_send_text(
              out, (slug_ffi_text){line, (uint64_t)length}, free)) return;
      line = NULL;
      length = 0;
      capacity = 0;
      continue;
    }

    if (length == capacity) {
      size_t next_capacity = capacity == 0 ? 128 : capacity * 2;
      char *expanded;
      if (next_capacity <= capacity) {
        free(line);
        return;
      }
      expanded = realloc(line, next_capacity);
      if (expanded == NULL) {
        free(line);
        return;
      }
      line = expanded;
      capacity = next_capacity;
    }
    line[length++] = (char)character;
  }

  if (ferror(stdin)) {
    free(line);
    return;
  }
  if (line != NULL) {
    slug_ffi_async_sender_send_text(out, (slug_ffi_text){line, (uint64_t)length}, free);
  }
}

static int32_t read_lines(const slug_ffi_host_api *host, slug_ffi_call *call,
                          void *raw_state) {
  core_state *state = raw_state;
  if (state != NULL && slug_ffi_async_stream_set_result(state->stdin, call)) {
    return SLUG_FFI_OK;
  }
  host->set_error(call, TEXT("native.io"), TEXT("cannot create standard-input stream"));
  return SLUG_FFI_ERROR;
}

static int32_t add(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  int64_t left, right;
  (void)state;
  if (!host->argument_i64(call, 0, &left) || !host->argument_i64(call, 1, &right)) {
    return SLUG_FFI_ERROR;
  }
  if ((right > 0 && left > INT64_MAX - right) ||
      (right < 0 && left < INT64_MIN - right)) {
    host->set_error(call, TEXT("math.range"), TEXT("integer addition overflowed"));
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, left + right);
  return SLUG_FFI_OK;
}

static int32_t square_root(const slug_ffi_host_api *host, slug_ffi_call *call,
                           void *state) {
  double value;
  (void)state;
  if (!host->argument_f64(call, 0, &value)) return SLUG_FFI_ERROR;
  if (value < 0.0) {
    host->set_error(call, TEXT("math.domain"),
                    TEXT("sqrt requires a non-negative number"));
    return SLUG_FFI_ERROR;
  }
  host->set_f64(call, sqrt(value));
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor STD_FUNCTIONS[] = {
    {sizeof(slug_ffi_function_descriptor), TEXT("keys"), TEXT("std.keys/v1"), 1, 1, keys},
};

static const slug_ffi_function_descriptor STDIN_FUNCTIONS[] = {
    {sizeof(slug_ffi_function_descriptor), TEXT("readLines"),
     TEXT("stdin.read_lines/v1"), 0, 0, read_lines},
};

static const slug_ffi_function_descriptor MATH_FUNCTIONS[] = {
    {sizeof(slug_ffi_function_descriptor), TEXT("add"), TEXT("math.add/v1"), 2, 2, add},
    {sizeof(slug_ffi_function_descriptor), TEXT("sqrt"), TEXT("math.sqrt/v1"), 1, 1,
     square_root},
};

static const slug_ffi_module_descriptor MODULES[] = {
    {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR,
     sizeof(slug_ffi_module_descriptor), TEXT("slug.std"), STD_FUNCTIONS, 1, NULL, 0},
    {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR,
     sizeof(slug_ffi_module_descriptor), TEXT("slug.io.stdin"), STDIN_FUNCTIONS, 1, NULL, 0},
    {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR,
     sizeof(slug_ffi_module_descriptor), TEXT("slug.math"), MATH_FUNCTIONS, 2, NULL, 0},
};

static void destroy_library(void *raw_state) {
  core_state *state = raw_state;
  if (state == NULL) return;
  slug_ffi_async_stream_destroy(state->stdin);
  free(state);
}

static const slug_ffi_library_descriptor LIBRARY = {
    SLUG_FFI_PROTOTYPE_ABI_MAJOR,
    SLUG_FFI_PROTOTYPE_ABI_MINOR,
    sizeof(slug_ffi_library_descriptor),
    destroy_library,
    MODULES,
    3,
};

SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_library_descriptor *slug_ffi_library_init(
    const slug_ffi_host_api *host, void **out_state) {
  core_state *state;
  if (host == NULL || host->abi_major != SLUG_FFI_PROTOTYPE_ABI_MAJOR ||
      host->table_size < sizeof(slug_ffi_host_api) || out_state == NULL) return NULL;
  state = calloc(1, sizeof(core_state));
  if (state == NULL) return NULL;
  state->stdin = slug_ffi_async_stream_create(
      host, STDIN_CHANNEL_CAPACITY, stdin_worker, NULL, NULL);
  if (state->stdin == NULL) {
    free(state);
    return NULL;
  }
  *out_state = state;
  return &LIBRARY;
}
