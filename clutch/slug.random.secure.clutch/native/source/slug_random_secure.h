#ifndef SLUG_RANDOM_SECURE_H
#define SLUG_RANDOM_SECURE_H

#include <stddef.h>
#include <stdint.h>

/* Fills `count` bytes, from 1 to 256, from the operating system's
 * cryptographically secure generator. Returns 0; 1 for a count out of range;
 * 2 when the operating system fails. The bytes stay in a per-thread buffer
 * until the thread's next call, and the host copies them at once. */
int32_t slug_random_secure_bytes(int64_t count, const uint8_t **data, size_t *length);

#endif
