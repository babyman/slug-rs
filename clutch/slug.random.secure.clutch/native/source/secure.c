#include "slug_random_secure.h"

#if defined(_WIN32)
#include <windows.h>
#include <bcrypt.h>
#elif defined(__APPLE__)
#include <stdlib.h>
#else
#include <errno.h>
#include <sys/random.h>
#endif

#define SLUG_RANDOM_SECURE_MAX 256

static _Thread_local uint8_t buffer[SLUG_RANDOM_SECURE_MAX];

int32_t slug_random_secure_bytes(int64_t count, const uint8_t **data, size_t *length) {
  *data = buffer;
  *length = 0;
  if (count < 1 || count > SLUG_RANDOM_SECURE_MAX) {
    return 1;
  }
#if defined(_WIN32)
  if (BCryptGenRandom(NULL, buffer, (ULONG)count, BCRYPT_USE_SYSTEM_PREFERRED_RNG) != 0) {
    return 2;
  }
#elif defined(__APPLE__)
  arc4random_buf(buffer, (size_t)count);
#else
  size_t filled = 0;
  while (filled < (size_t)count) {
    ssize_t got = getrandom(buffer + filled, (size_t)count - filled, 0);
    if (got < 0) {
      if (errno == EINTR) {
        continue;
      }
      return 2;
    }
    filled += (size_t)got;
  }
#endif
  *length = (size_t)count;
  return 0;
}
