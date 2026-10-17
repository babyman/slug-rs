#ifndef SLUG_CORE_H
#define SLUG_CORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "slug_producer.h"

/* slug_core_start_reader results. */
#define SLUG_CORE_READER_STARTED 0
#define SLUG_CORE_READER_RUNNING 1
#define SLUG_CORE_READER_FAILED 2

/* Starts the one standard-input reader thread, which sends each line as text
 * through `lines` and closes it at end of input. Takes ownership of `lines`:
 * the reader releases it when it stops, and an early return releases it
 * immediately. A second call returns SLUG_CORE_READER_RUNNING. */
int32_t slug_core_start_reader(slug_producer *lines);

/* Writes `left + right` to `sum` and returns true, or returns false when the
 * sum does not fit in an int64_t. */
bool slug_core_add(int64_t left, int64_t right, int64_t *sum);

/* Returns the square root of `value`. */
double slug_core_sqrt(double value);

/* Returns the Unicode scalar value of the first character of `text`, which is
 * valid UTF-8, or -1 when `length` is 0. */
int64_t slug_core_ord(const char *text, size_t length);

/* Encodes the Unicode scalar value `code` as UTF-8 into a per-thread buffer,
 * points `text` and `length` at it, and returns true. The buffer stays valid
 * until the thread's next call. Returns false, writing nothing, when `code` is
 * negative, a surrogate, or above U+10FFFF. */
bool slug_core_chr(int64_t code, const char **text, size_t *length);

#endif
