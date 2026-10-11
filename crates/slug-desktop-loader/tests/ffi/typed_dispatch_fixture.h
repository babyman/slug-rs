#ifndef SLUG_TYPED_DISPATCH_FIXTURE_H
#define SLUG_TYPED_DISPATCH_FIXTURE_H

#include <stdint.h>

typedef struct slug_spike_handle slug_spike_handle;

typedef enum {
  SLUG_SPIKE_OK = 0,
  SLUG_SPIKE_EOF = 1,
  SLUG_SPIKE_IO = 2,
} slug_spike_status;

slug_spike_handle *slug_spike_open_read(const char *path);
slug_spike_handle *slug_spike_open_write(const char *path);
slug_spike_handle *slug_spike_open_append(const char *path);
slug_spike_status slug_spike_read_line(slug_spike_handle *handle, const char **out_text);
int64_t slug_spike_write(slug_spike_handle *handle, const char *text, uint64_t length);
slug_spike_status slug_spike_close(slug_spike_handle *handle);
int64_t slug_spike_add(int64_t left, int64_t right);

#endif
