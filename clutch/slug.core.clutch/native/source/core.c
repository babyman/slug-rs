#if !defined(_WIN32)
/* nanosleep is POSIX; shims build with -std=c11. */
#define _POSIX_C_SOURCE 200809L
#endif

#include "slug_core.h"

#include <math.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#include <windows.h>
#else
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>
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

/* Whether `data` is well-formed UTF-8: no overlong forms, surrogates, or
 * values above U+10FFFF. */
static bool valid_utf8(const uint8_t *data, size_t length) {
  size_t i = 0;
  while (i < length) {
    uint8_t lead = data[i];
    size_t size;
    uint8_t low = 0x80, high = 0xBF;
    if (lead < 0x80) {
      i++;
      continue;
    } else if (lead >= 0xC2 && lead <= 0xDF) {
      size = 2;
    } else if (lead >= 0xE0 && lead <= 0xEF) {
      size = 3;
      if (lead == 0xE0) low = 0xA0;
      if (lead == 0xED) high = 0x9F;
    } else if (lead >= 0xF0 && lead <= 0xF4) {
      size = 4;
      if (lead == 0xF0) low = 0x90;
      if (lead == 0xF4) high = 0x8F;
    } else {
      return false;
    }
    if (length - i < size) return false;
    if (data[i + 1] < low || data[i + 1] > high) return false;
    for (size_t k = 2; k < size; k++) {
      if (data[i + k] < 0x80 || data[i + k] > 0xBF) return false;
    }
    i += size;
  }
  return true;
}

bool slug_core_from_bytes(const uint8_t *data, size_t data_length, const char **text,
                          size_t *length) {
  if (!valid_utf8(data, data_length)) return false;
  *text = data_length == 0 ? "" : (const char *)data;
  *length = data_length;
  return true;
}

void slug_core_to_bytes(const char *text, size_t text_length, const uint8_t **data,
                        size_t *length) {
  static const uint8_t empty[1] = {0};
  *data = text_length == 0 ? empty : (const uint8_t *)text;
  *length = text_length;
}

int64_t slug_core_find(const uint8_t *data, size_t data_length, const uint8_t *needle,
                       size_t needle_length, int64_t from) {
  if (from < 0 || (uint64_t)from > data_length) return -1;
  size_t at = (size_t)from;
  if (needle_length == 0) return (int64_t)at;
  while (data_length - at >= needle_length) {
    /* The first byte by memchr, then the rest. */
    const uint8_t *first = memchr(data + at, needle[0], data_length - at - needle_length + 1);
    if (first == NULL) return -1;
    at = (size_t)(first - data);
    if (memcmp(first + 1, needle + 1, needle_length - 1) == 0) return (int64_t)at;
    at += 1;
  }
  return -1;
}

/* --- Signals ------------------------------------------------------------
 *
 * Each call to slug_core_notify_signals adds a listener: the kinds it wants
 * and the producer to send their names through. A signal goes to every
 * listener for its kind. Once a signal of a kind has been delivered, the
 * next one of that kind ends the process as if no one listened, until a
 * new call asks for that kind again; and a signal that reaches no open
 * listener ends the process too. */

#define SIGNAL_KINDS 2
static const char *const signal_names[SIGNAL_KINDS] = {"int", "term"};

typedef struct {
  int64_t kinds;
  slug_producer *events;
} signal_listener;

static signal_listener *listeners = NULL;
static size_t listener_count = 0;
static size_t listener_capacity = 0;

#if defined(_WIN32)
static SRWLOCK listeners_lock = SRWLOCK_INIT;
static void lock_listeners(void) { AcquireSRWLockExclusive(&listeners_lock); }
static void unlock_listeners(void) { ReleaseSRWLockExclusive(&listeners_lock); }
#else
static pthread_mutex_t listeners_lock = PTHREAD_MUTEX_INITIALIZER;
static void lock_listeners(void) { pthread_mutex_lock(&listeners_lock); }
static void unlock_listeners(void) { pthread_mutex_unlock(&listeners_lock); }
#endif

/* Sends the name of signal `kind` to every listener for it, dropping those
 * whose channel has gone; returns whether any listener took it. A listener
 * whose channel is full already has a signal waiting, so it counts. */
static bool deliver_signal(int kind) {
  const char *name = signal_names[kind];
  size_t length = kind == 0 ? 3 : 4;
  bool taken = false;
  lock_listeners();
  size_t i = 0;
  while (i < listener_count) {
    signal_listener *listener = &listeners[i];
    if ((listener->kinds & ((int64_t)1 << kind)) == 0) {
      i++;
      continue;
    }
    int32_t status = slug_producer_send_text(listener->events, name, length);
    if (status == SLUG_PRODUCER_SENT || status == SLUG_PRODUCER_FULL) {
      taken = true;
      i++;
    } else {
      slug_producer_release(listener->events);
      listeners[i] = listeners[--listener_count];
    }
  }
  unlock_listeners();
  return taken;
}

static bool add_listener(int64_t kinds, slug_producer *events) {
  lock_listeners();
  if (listener_count == listener_capacity) {
    size_t capacity = listener_capacity == 0 ? 4 : listener_capacity * 2;
    signal_listener *grown = realloc(listeners, capacity * sizeof *grown);
    if (grown == NULL) {
      unlock_listeners();
      return false;
    }
    listeners = grown;
    listener_capacity = capacity;
  }
  listeners[listener_count++] = (signal_listener){kinds, events};
  unlock_listeners();
  return true;
}

#if defined(_WIN32)

/* Ctrl-C is "int" and Ctrl-Break "term"; the console runs this handler on a
 * thread of its own. Returning FALSE lets the default handler end the
 * process. */
static volatile LONG signal_delivered[SIGNAL_KINDS];
static volatile LONG console_handler_installed = 0;

static BOOL WINAPI on_console_event(DWORD type) {
  int kind = type == CTRL_C_EVENT ? 0 : type == CTRL_BREAK_EVENT ? 1 : -1;
  if (kind < 0) return FALSE;
  if (InterlockedExchange(&signal_delivered[kind], 1) != 0) return FALSE;
  return deliver_signal(kind) ? TRUE : FALSE;
}

static bool watch_signals(int64_t kinds) {
  for (int kind = 0; kind < SIGNAL_KINDS; kind++) {
    if (kinds & ((int64_t)1 << kind)) InterlockedExchange(&signal_delivered[kind], 0);
  }
  if (InterlockedCompareExchange(&console_handler_installed, 1, 0) == 0) {
    if (!SetConsoleCtrlHandler(on_console_event, TRUE)) {
      InterlockedExchange(&console_handler_installed, 0);
      return false;
    }
  }
  return true;
}

#else

/* The handler only writes the signal's number to a pipe, which is safe in a
 * signal handler, and puts back the default action, so a second signal
 * ends the process however busy the program is. A watcher thread reads the
 * pipe and does the rest. */
static const int signal_numbers[SIGNAL_KINDS] = {SIGINT, SIGTERM};
static int signal_pipe[2] = {-1, -1};
static pthread_once_t watcher_once = PTHREAD_ONCE_INIT;
static bool watcher_running = false;

static void on_signal(int number) {
  int saved = errno;
  struct sigaction fallback;
  fallback.sa_handler = SIG_DFL;
  fallback.sa_flags = 0;
  sigemptyset(&fallback.sa_mask);
  sigaction(number, &fallback, NULL);
  unsigned char byte = (unsigned char)number;
  ssize_t written = write(signal_pipe[1], &byte, 1);
  (void)written;
  errno = saved;
}

static void *signal_watcher(void *unused) {
  (void)unused;
  for (;;) {
    unsigned char byte;
    ssize_t count = read(signal_pipe[0], &byte, 1);
    if (count < 0 && errno == EINTR) continue;
    if (count <= 0) return NULL;
    int kind = byte == SIGINT ? 0 : byte == SIGTERM ? 1 : -1;
    /* No one took it: the default action is back, so this ends the process. */
    if (kind >= 0 && !deliver_signal(kind)) kill(getpid(), byte);
  }
}

static void start_watcher(void) {
  if (pipe(signal_pipe) != 0) return;
  for (int i = 0; i < 2; i++) fcntl(signal_pipe[i], F_SETFD, FD_CLOEXEC);
  fcntl(signal_pipe[1], F_SETFL, fcntl(signal_pipe[1], F_GETFL) | O_NONBLOCK);
  pthread_t thread;
  if (pthread_create(&thread, NULL, signal_watcher, NULL) != 0) return;
  pthread_detach(thread);
  watcher_running = true;
}

static bool watch_signals(int64_t kinds) {
  pthread_once(&watcher_once, start_watcher);
  if (!watcher_running) return false;
  for (int kind = 0; kind < SIGNAL_KINDS; kind++) {
    if ((kinds & ((int64_t)1 << kind)) == 0) continue;
    struct sigaction action;
    action.sa_handler = on_signal;
    action.sa_flags = SA_RESTART;
    sigemptyset(&action.sa_mask);
    if (sigaction(signal_numbers[kind], &action, NULL) != 0) return false;
  }
  return true;
}

#endif

int32_t slug_core_notify_signals(int64_t kinds, slug_producer *events) {
  if (kinds <= 0 || kinds >= ((int64_t)1 << SIGNAL_KINDS) || !add_listener(kinds, events)) {
    slug_producer_release(events);
    return SLUG_CORE_SIGNALS_FAILED;
  }
  /* The listener stays registered even if watching fails; a signal then
   * takes its default action, and the listener is dropped once its channel
   * goes. */
  return watch_signals(kinds) ? SLUG_CORE_SIGNALS_WATCHING : SLUG_CORE_SIGNALS_FAILED;
}
