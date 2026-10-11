#include "typed_dispatch_fixture.h"
#include "typed_shim_abi.h"

#include <inttypes.h>
#include <stdio.h>
#include <time.h>

const slug_shim_table *slug_shim_get_table(void);

static uint64_t elapsed_ns(struct timespec start, struct timespec end) {
  return (uint64_t)(end.tv_sec - start.tv_sec) * UINT64_C(1000000000) + (uint64_t)(end.tv_nsec - start.tv_nsec);
}

int main(void) {
  const uint64_t calls = UINT64_C(100000000);
  const slug_shim_table *table = slug_shim_get_table();
  const slug_shim_entry shim = table->bindings[6].entry;
  volatile int64_t sink = 0;
  struct timespec start, end;
  slug_value arguments[2] = {{{.i64 = 1}, 0}, {{.i64 = 2}, 0}};
  slug_value results[1] = {{{.i64 = 0}, 0}};
  slug_shim_cleanup cleanup = {0};

  clock_gettime(CLOCK_MONOTONIC, &start);
  for (uint64_t index = 0; index < calls; index++) sink += slug_spike_add((int64_t)index, 1);
  clock_gettime(CLOCK_MONOTONIC, &end);
  printf("plain-c-direct calls=%" PRIu64 " total_ns=%" PRIu64 " sink=%" PRId64 "\n", calls, elapsed_ns(start, end), sink);

  sink = 0;
  clock_gettime(CLOCK_MONOTONIC, &start);
  for (uint64_t index = 0; index < calls; index++) {
    arguments[0].as.i64 = (int64_t)index;
    shim(arguments, results, &cleanup);
    sink += results[0].as.i64;
  }
  clock_gettime(CLOCK_MONOTONIC, &end);
  printf("plain-c-uniform-shim calls=%" PRIu64 " total_ns=%" PRIu64 " sink=%" PRId64 "\n", calls, elapsed_ns(start, end), sink);
  return 0;
}
