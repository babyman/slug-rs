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

/* slug_core_notify_signals kinds, as bits, and results. */
#define SLUG_CORE_SIGNAL_INT 1
#define SLUG_CORE_SIGNAL_TERM 2
#define SLUG_CORE_SIGNALS_WATCHING 0
#define SLUG_CORE_SIGNALS_FAILED 1

/* Sends "int" through `events` for each interrupt (SIGINT, or Ctrl-C on
 * Windows) and "term" for each request to terminate (SIGTERM, or Ctrl-Break
 * on Windows), as `kinds` asks, instead of their default action. After one
 * is delivered, the next of its kind takes the default action, until a new
 * call asks for that kind again; so does one that reaches no open channel.
 * Takes ownership of `events`, which is released once its channel goes. */
int32_t slug_core_notify_signals(int64_t kinds, slug_producer *events);

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

/* Points `text` and `length` at `data` and returns true when `data` is valid
 * UTF-8, or returns false, writing nothing, when it is not. */
bool slug_core_from_bytes(const uint8_t *data, size_t data_length, const char **text,
                          size_t *length);

/* Points `data` and `length` at the UTF-8 bytes of `text`. */
void slug_core_to_bytes(const char *text, size_t text_length, const uint8_t **data,
                        size_t *length);

/* The offset of the first occurrence of `needle` in `data` at or after
 * offset `from`, -1 when there is none, or -2 when `from` is out of range.
 * An empty needle is found at `from`. */
int64_t slug_core_find(const uint8_t *data, size_t data_length, const uint8_t *needle,
                       size_t needle_length, int64_t from);

/* The offset of the first byte of `data` at or after offset `from` that is
 * one of the bytes of `set`, -1 when there is none, or -2 when `from` is
 * out of range. */
int64_t slug_core_find_any(const uint8_t *data, size_t data_length, const uint8_t *set,
                           size_t set_length, int64_t from);

#endif
