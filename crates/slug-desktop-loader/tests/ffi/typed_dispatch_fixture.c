#include "typed_dispatch_fixture.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct slug_spike_handle {
  FILE *stream;
  char *line;
  size_t capacity;
};

static slug_spike_handle *open_file(const char *path, const char *mode) {
  slug_spike_handle *handle;
  if (strcmp(path, "native-error") == 0) return NULL;
  handle = calloc(1, sizeof(*handle));
  if (handle == NULL) return NULL;
  handle->stream = fopen(path, mode);
  if (handle->stream == NULL) {
    free(handle);
    return NULL;
  }
  return handle;
}

slug_spike_handle *slug_spike_open_read(const char *path) { return open_file(path, "rb"); }
slug_spike_handle *slug_spike_open_write(const char *path) { return open_file(path, "wb"); }
slug_spike_handle *slug_spike_open_append(const char *path) { return open_file(path, "ab"); }

slug_spike_status slug_spike_read_line(slug_spike_handle *handle, const char **out_text) {
  size_t length = 0;
  int character;
  if (handle == NULL || handle->stream == NULL || out_text == NULL) return SLUG_SPIKE_IO;
  *out_text = NULL;
  while ((character = fgetc(handle->stream)) != EOF) {
    if (character == '\n') break;
    if (length + 1 >= handle->capacity) {
      size_t capacity = handle->capacity == 0 ? 64 : handle->capacity * 2;
      char *line = realloc(handle->line, capacity);
      if (line == NULL) return SLUG_SPIKE_IO;
      handle->line = line;
      handle->capacity = capacity;
    }
    handle->line[length++] = (char)character;
  }
  if (ferror(handle->stream)) return SLUG_SPIKE_IO;
  if (character == EOF && length == 0) return SLUG_SPIKE_EOF;
  if (length + 1 >= handle->capacity) {
    char *line = realloc(handle->line, length + 1);
    if (line == NULL) return SLUG_SPIKE_IO;
    handle->line = line;
    handle->capacity = length + 1;
  }
  handle->line[length] = '\0';
  *out_text = handle->line;
  return SLUG_SPIKE_OK;
}

int64_t slug_spike_write(slug_spike_handle *handle, const char *text, uint64_t length) {
  if (handle == NULL || handle->stream == NULL || text == NULL) return -1;
  if (fwrite(text, 1, (size_t)length, handle->stream) != length || fflush(handle->stream) != 0) return -1;
  return (int64_t)length;
}

slug_spike_status slug_spike_close(slug_spike_handle *handle) {
  int result;
  if (handle == NULL) return SLUG_SPIKE_OK;
  result = handle->stream == NULL ? 0 : fclose(handle->stream);
  handle->stream = NULL;
  free(handle->line);
  free(handle);
  return result == 0 ? SLUG_SPIKE_OK : SLUG_SPIKE_IO;
}

int64_t slug_spike_add(int64_t left, int64_t right) { return left + right; }
