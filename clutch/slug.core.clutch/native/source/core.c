#if !defined(_WIN32)
/* nanosleep is POSIX; shims build with -std=c11. */
#define _POSIX_C_SOURCE 200809L
#endif

#include "slug_core.h"

#include <math.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>

#if defined(_WIN32)
#include <windows.h>
#else
#include <pthread.h>
#include <time.h>
#endif

static atomic_bool reader_started = false;

static void pause_briefly(void) {
#if defined(_WIN32)
  Sleep(1);
#else
  struct timespec delay = {0, 1000000};
  nanosleep(&delay, NULL);
#endif
}

/* Sends one line, retrying while the channel is full. Returns false when the
 * reader must stop. */
static bool send_line(slug_producer *lines, const char *line, size_t length) {
  for (;;) {
    switch (slug_producer_send_text(lines, line, length)) {
      case SLUG_PRODUCER_SENT:
        return true;
      case SLUG_PRODUCER_FULL:
        pause_briefly();
        break;
      default:
        return false;
    }
  }
}

static void read_lines(slug_producer *lines) {
  char *line = NULL;
  size_t length = 0;
  size_t capacity = 0;
  int character;

  while ((character = fgetc(stdin)) != EOF) {
    if (character == '\n') {
      if (length > 0 && line[length - 1] == '\r') length--;
      if (!send_line(lines, line == NULL ? "" : line, length)) break;
      length = 0;
      continue;
    }
    if (length == capacity) {
      size_t next_capacity = capacity == 0 ? 128 : capacity * 2;
      char *expanded = next_capacity > capacity ? realloc(line, next_capacity) : NULL;
      if (expanded == NULL) break;
      line = expanded;
      capacity = next_capacity;
    }
    line[length++] = (char)character;
  }
  /* An unterminated final line is still a line. */
  if (feof(stdin) && !ferror(stdin) && length > 0) send_line(lines, line, length);
  free(line);
  slug_producer_close(lines);
  slug_producer_release(lines);
}

#if defined(_WIN32)
static DWORD WINAPI reader_thread(LPVOID lines) {
  read_lines(lines);
  return 0;
}
#else
static void *reader_thread(void *lines) {
  read_lines(lines);
  return NULL;
}
#endif

int32_t slug_core_start_reader(slug_producer *lines) {
  if (atomic_exchange(&reader_started, true)) {
    slug_producer_release(lines);
    return SLUG_CORE_READER_RUNNING;
  }
#if defined(_WIN32)
  HANDLE thread = CreateThread(NULL, 0, reader_thread, lines, 0, NULL);
  if (thread != NULL) {
    CloseHandle(thread);
    return SLUG_CORE_READER_STARTED;
  }
#else
  pthread_t thread;
  if (pthread_create(&thread, NULL, reader_thread, lines) == 0) {
    pthread_detach(thread);
    return SLUG_CORE_READER_STARTED;
  }
#endif
  atomic_store(&reader_started, false);
  slug_producer_release(lines);
  return SLUG_CORE_READER_FAILED;
}

bool slug_core_add(int64_t left, int64_t right, int64_t *sum) {
  if ((right > 0 && left > INT64_MAX - right) || (right < 0 && left < INT64_MIN - right)) {
    return false;
  }
  *sum = left + right;
  return true;
}

double slug_core_sqrt(double value) { return sqrt(value); }

int64_t slug_core_ord(const char *text, size_t length) {
  if (length == 0) return -1;
  const unsigned char *bytes = (const unsigned char *)text;
  /* Slug strings are valid UTF-8, so the lead byte gives the sequence length
   * and every continuation byte is present. */
  if (bytes[0] < 0x80) return bytes[0];
  if (bytes[0] < 0xE0) return ((int64_t)(bytes[0] & 0x1F) << 6) | (bytes[1] & 0x3F);
  if (bytes[0] < 0xF0) {
    return ((int64_t)(bytes[0] & 0x0F) << 12) | ((int64_t)(bytes[1] & 0x3F) << 6) |
           (bytes[2] & 0x3F);
  }
  return ((int64_t)(bytes[0] & 0x07) << 18) | ((int64_t)(bytes[1] & 0x3F) << 12) |
         ((int64_t)(bytes[2] & 0x3F) << 6) | (bytes[3] & 0x3F);
}

bool slug_core_chr(int64_t code, const char **text, size_t *length) {
  static _Thread_local char buffer[4];
  if (code < 0 || code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF)) return false;
  size_t size = 0;
  if (code < 0x80) {
    buffer[size++] = (char)code;
  } else if (code < 0x800) {
    buffer[size++] = (char)(0xC0 | (code >> 6));
    buffer[size++] = (char)(0x80 | (code & 0x3F));
  } else if (code < 0x10000) {
    buffer[size++] = (char)(0xE0 | (code >> 12));
    buffer[size++] = (char)(0x80 | ((code >> 6) & 0x3F));
    buffer[size++] = (char)(0x80 | (code & 0x3F));
  } else {
    buffer[size++] = (char)(0xF0 | (code >> 18));
    buffer[size++] = (char)(0x80 | ((code >> 12) & 0x3F));
    buffer[size++] = (char)(0x80 | ((code >> 6) & 0x3F));
    buffer[size++] = (char)(0x80 | (code & 0x3F));
  }
  *text = buffer;
  *length = size;
  return true;
}
