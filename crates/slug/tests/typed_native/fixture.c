#if defined(_WIN32)
#define _CRT_SECURE_NO_WARNINGS
#endif

#include "fixture.h"

#include <stdio.h>
#include <string.h>

struct fx_file {
  FILE *stream;
  char line[256];
};

static fx_file *open_file(const char *path, const char *mode) {
  fx_file *file = calloc(1, sizeof *file);
  if (file == NULL) return NULL;
  file->stream = fopen(path, mode);
  if (file->stream == NULL) {
    free(file);
    return NULL;
  }
  return file;
}

fx_file *fx_open_read(const char *path) { return open_file(path, "rb"); }
fx_file *fx_open_write(const char *path) { return open_file(path, "wb"); }

int32_t fx_read_line(fx_file *file, const char **line) {
  size_t length;
  if (fgets(file->line, (int)sizeof file->line, file->stream) == NULL) {
    return ferror(file->stream) ? FX_IO : FX_EOF;
  }
  length = strlen(file->line);
  if (length > 0 && file->line[length - 1] == '\n') file->line[length - 1] = '\0';
  *line = file->line;
  return FX_OK;
}

int64_t fx_write(fx_file *file, const char *text, size_t length) {
  if (fwrite(text, 1, length, file->stream) != length || fflush(file->stream) != 0) return -1;
  return (int64_t)length;
}

void fx_close(fx_file *file) {
  fclose(file->stream);
  free(file);
}

int64_t fx_add(int64_t left, int64_t right) { return left + right; }

int8_t fx_negate(int8_t value) { return (int8_t)-value; }

char *fx_describe(int64_t value) {
  char *text = malloc(32);
  if (text != NULL) snprintf(text, 32, "<%lld>", (long long)value);
  return text;
}

const char *fx_label(int64_t value) { return value > 0 ? "positive" : NULL; }
