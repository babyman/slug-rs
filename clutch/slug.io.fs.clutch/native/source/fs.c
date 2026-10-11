#if defined(_WIN32)
#define _CRT_SECURE_NO_WARNINGS
#endif

#include "slug_io_fs.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Bounds native memory per line. */
#define MAX_LINE_BYTES ((size_t)16u * 1024u * 1024u)

struct slug_fs_file {
  FILE *stream;
  char *line;
  size_t capacity;
};

static slug_fs_file *open_file(const char *path, const char *mode) {
  slug_fs_file *file = calloc(1, sizeof *file);
  if (file == NULL) return NULL;
  file->stream = fopen(path, mode);
  if (file->stream == NULL) {
    free(file);
    return NULL;
  }
  return file;
}

slug_fs_file *slug_fs_open_read(const char *path) { return open_file(path, "rb"); }
slug_fs_file *slug_fs_open_write(const char *path) { return open_file(path, "wb"); }
slug_fs_file *slug_fs_open_append(const char *path) { return open_file(path, "ab"); }

/* Grows the line buffer to hold at least `needed` bytes. */
static int32_t reserve(slug_fs_file *file, size_t needed) {
  size_t capacity;
  char *expanded;
  if (needed <= file->capacity) return SLUG_FS_OK;
  capacity = file->capacity == 0 ? 128 : file->capacity;
  while (capacity < needed) {
    if (capacity > MAX_LINE_BYTES) return SLUG_FS_LINE_GROW_FAILED;
    capacity *= 2;
  }
  expanded = realloc(file->line, capacity);
  if (expanded == NULL) {
    return file->capacity == 0 ? SLUG_FS_LINE_ALLOC_FAILED : SLUG_FS_LINE_GROW_FAILED;
  }
  file->line = expanded;
  file->capacity = capacity;
  return SLUG_FS_OK;
}

int32_t slug_fs_read_line(slug_fs_file *file, const char **line) {
  size_t length = 0;
  int character = 0;
  int32_t status = reserve(file, 1);
  if (status != SLUG_FS_OK) return status;
  while ((character = fgetc(file->stream)) != EOF && character != '\n') {
    if (length >= MAX_LINE_BYTES) return SLUG_FS_LINE_TOO_LONG;
    if (character == '\0') return SLUG_FS_LINE_HAS_NUL;
    status = reserve(file, length + 2);
    if (status != SLUG_FS_OK) return status;
    file->line[length++] = (char)character;
  }
  if (ferror(file->stream)) return SLUG_FS_READ_FAILED;
  if (character == EOF && length == 0) return SLUG_FS_EOF;
  if (length > 0 && file->line[length - 1] == '\r') length--;
  file->line[length] = '\0';
  *line = file->line;
  return SLUG_FS_OK;
}

int64_t slug_fs_write(slug_fs_file *file, const char *text, size_t length) {
  if (fwrite(text, 1, length, file->stream) != length) return SLUG_FS_WRITE_FAILED;
  if (fflush(file->stream) != 0) return SLUG_FS_FLUSH_FAILED;
  return (int64_t)length;
}

void slug_fs_close(slug_fs_file *file) {
  fclose(file->stream);
  free(file->line);
  free(file);
}
