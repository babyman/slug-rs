#ifndef FIXTURE_BETA_H
#define FIXTURE_BETA_H
#include <stdint.h>
typedef struct beta_counter beta_counter;
beta_counter *beta_open(int64_t start);
int64_t beta_next(beta_counter *counter);
void beta_close(beta_counter *counter);
const char *beta_name(void);
#endif
