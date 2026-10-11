#include "beta.h"

#include <stdlib.h>

struct beta_counter {
  int64_t value;
};

beta_counter *beta_open(int64_t start) {
  beta_counter *counter = malloc(sizeof *counter);
  if (counter != NULL) counter->value = start;
  return counter;
}

int64_t beta_next(beta_counter *counter) { return ++counter->value; }

void beta_close(beta_counter *counter) { free(counter); }

const char *beta_name(void) { return "beta"; }
