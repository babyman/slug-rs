#include "slug_ffi_prototype.h" /* Version-0 clutch native source. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})
#define MAX_LINE_BYTES (16u * 1024u * 1024u)

static void destroy_file(void *resource) {
  if (resource != NULL) fclose((FILE *)resource);
}

static int32_t open_file(const slug_ffi_host_api *host, slug_ffi_call *call,
                         const char *mode) {
  slug_ffi_text path_text = {0};
  if (!host->argument_text(call, 0, &path_text)) return SLUG_FFI_ERROR;
  if (path_text.length > SIZE_MAX - 1) {
    host->set_error(call, TEXT("native.io"), TEXT("file path is too large"));
    return SLUG_FFI_ERROR;
  }
  size_t path_length = (size_t)path_text.length;
  char *path = malloc(path_length + 1);
  if (path == NULL) {
    host->set_error(call, TEXT("native.alloc"), TEXT("cannot allocate file path"));
    return SLUG_FFI_ERROR;
  }
  memcpy(path, path_text.data, path_length);
  path[path_length] = '\0';
  FILE *file = fopen(path, mode);
  if (file == NULL) {
    free(path);
    host->set_error(call, TEXT("native.io"), TEXT("cannot open file"));
    return SLUG_FFI_ERROR;
  }
  free(path);
  if (!host->set_resource(call, TEXT("File"), file)) {
    fclose(file);
    return SLUG_FFI_ERROR;
  }
  return SLUG_FFI_OK;
}

static int32_t open_read(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  return open_file(host, call, "rb");
}

static int32_t open_write(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  return open_file(host, call, "wb");
}

static int32_t open_append(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  return open_file(host, call, "ab");
}

static int32_t read_line(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *resource = NULL;
  if (!host->argument_resource(call, 0, TEXT("File"), &resource)) return SLUG_FFI_ERROR;
  FILE *file = resource;
  size_t length = 0;
  size_t capacity = 128;
  char *line = malloc(capacity);
  if (line == NULL) {
    host->set_error(call, TEXT("native.alloc"), TEXT("cannot allocate line buffer"));
    return SLUG_FFI_ERROR;
  }
  int character = 0;
  while ((character = fgetc(file)) != EOF && character != '\n') {
    if (length >= MAX_LINE_BYTES) {
      free(line);
      host->set_error(call, TEXT("native.io"), TEXT("file line exceeds 16 MiB limit"));
      return SLUG_FFI_ERROR;
    }
    if (length == capacity) {
      if (capacity > SIZE_MAX / 2 || capacity > MAX_LINE_BYTES / 2) {
        free(line);
        host->set_error(call, TEXT("native.alloc"), TEXT("cannot grow line buffer"));
        return SLUG_FFI_ERROR;
      }
      capacity *= 2;
      char *expanded = realloc(line, capacity);
      if (expanded == NULL) {
        free(line);
        host->set_error(call, TEXT("native.alloc"), TEXT("cannot grow line buffer"));
        return SLUG_FFI_ERROR;
      }
      line = expanded;
    }
    line[length++] = (char)character;
  }
  if (ferror(file)) {
    free(line);
    host->set_error(call, TEXT("native.io"), TEXT("cannot read file"));
    return SLUG_FFI_ERROR;
  }
  if (character == EOF && length == 0) {
    free(line);
    host->set_nil(call);
    return SLUG_FFI_OK;
  }
  if (length > 0 && line[length - 1] == '\r') length--;
  slug_ffi_text text = {line, length};
  int success = host->set_text(call, text);
  free(line);
  return success ? SLUG_FFI_OK : SLUG_FFI_ERROR;
}

static int32_t write_file(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *resource = NULL;
  slug_ffi_text text = {0};
  if (!host->argument_resource(call, 0, TEXT("File"), &resource) ||
      !host->argument_text(call, 1, &text)) return SLUG_FFI_ERROR;
  if (fwrite(text.data, 1, (size_t)text.length, (FILE *)resource) != text.length) {
    host->set_error(call, TEXT("native.io"), TEXT("cannot write file"));
    return SLUG_FFI_ERROR;
  }
  if (fflush((FILE *)resource) != 0) {
    host->set_error(call, TEXT("native.io"), TEXT("cannot flush file"));
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, (int64_t)text.length);
  return SLUG_FFI_OK;
}

static int32_t close_file(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  if (!host->close_resource(call, 0, TEXT("File"))) return SLUG_FFI_ERROR;
  host->set_nil(call);
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("openRead"), TEXT("fs.openRead/v1"), 1, 1, open_read},
  {sizeof(slug_ffi_function_descriptor), TEXT("openWrite"), TEXT("fs.openWrite/v1"), 1, 1, open_write},
  {sizeof(slug_ffi_function_descriptor), TEXT("openAppend"), TEXT("fs.openAppend/v1"), 1, 1, open_append},
  {sizeof(slug_ffi_function_descriptor), TEXT("readLine"), TEXT("fs.readLine/v1"), 1, 1, read_line},
  {sizeof(slug_ffi_function_descriptor), TEXT("write"), TEXT("fs.write/v1"), 2, 2, write_file},
  {sizeof(slug_ffi_function_descriptor), TEXT("close"), TEXT("fs.close/v1"), 1, 1, close_file},
};

static const slug_ffi_resource_descriptor RESOURCES[] = {
  {sizeof(slug_ffi_resource_descriptor), TEXT("File"), destroy_file},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  TEXT("slug.io.fs"),
  NULL,
  FUNCTIONS,
  6,
  RESOURCES,
  1,
};

SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_module_descriptor *slug_ffi_module_init(
    const slug_ffi_host_api *host, void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &MODULE;
}
