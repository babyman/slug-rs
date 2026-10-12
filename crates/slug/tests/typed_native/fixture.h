#ifndef SLUG_TYPED_NATIVE_FIXTURE_H
#define SLUG_TYPED_NATIVE_FIXTURE_H

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct fx_file fx_file;

/* Status values returned by fx_read_line. */
#define FX_OK 0
#define FX_EOF 1
#define FX_IO 2

fx_file *fx_open_read(const char *path);
fx_file *fx_open_write(const char *path);
int32_t fx_read_line(fx_file *file, const char **line);
int64_t fx_write(fx_file *file, const char *text, size_t length);
void fx_close(fx_file *file);
/* Counts files closed explicitly or released by the host. */
int64_t fx_closed_count(void);
int64_t fx_add(int64_t left, int64_t right);
int8_t fx_negate(int8_t value);
char *fx_describe(int64_t value);
const char *fx_label(int64_t value);

#endif
