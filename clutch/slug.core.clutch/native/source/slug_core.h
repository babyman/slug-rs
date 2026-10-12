#ifndef SLUG_CORE_H
#define SLUG_CORE_H

#include <stdbool.h>
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

#endif
