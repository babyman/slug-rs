#include "alpha.h"

#include <string.h>

int64_t alpha_add(int64_t left, int64_t right) { return left + right; }

char *alpha_greet(const char *name) {
  size_t length = strlen(name);
  char *text = malloc(length + 8);
  if (text == NULL) return NULL;
  memcpy(text, "hello, ", 7);
  memcpy(text + 7, name, length + 1);
  return text;
}

/* Both shims bind a function named `name`; their entries must not collide. */
const char *alpha_name(void) { return "alpha"; }
