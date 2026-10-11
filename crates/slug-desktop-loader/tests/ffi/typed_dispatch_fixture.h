#ifndef SLUG_TYPED_DISPATCH_FIXTURE_H
#define SLUG_TYPED_DISPATCH_FIXTURE_H

#include <stdint.h>

#if defined(_WIN32)
#define SLUG_TYPED_DISPATCH_FIXTURE_EXPORT __declspec(dllexport)
#else
#define SLUG_TYPED_DISPATCH_FIXTURE_EXPORT
#endif

typedef struct slug_spike_handle slug_spike_handle;

typedef enum {
  SLUG_SPIKE_OK = 0,
  SLUG_SPIKE_EOF = 1,
  SLUG_SPIKE_IO = 2,
} slug_spike_status;

SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_handle *slug_spike_open_read(const char *path);
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_handle *slug_spike_open_write(const char *path);
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_handle *slug_spike_open_append(const char *path);
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_status slug_spike_read_line(
    slug_spike_handle *handle, const char **out_text);
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT int64_t slug_spike_write(
    slug_spike_handle *handle, const char *text, uint64_t length);
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT slug_spike_status slug_spike_close(slug_spike_handle *handle);
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT int64_t slug_spike_add(int64_t left, int64_t right);

#endif
