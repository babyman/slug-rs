#include "native/pulse.h"

int32_t pulse_count(int32_t requested) {
  if (requested < 1) return 1;
  if (requested > 8) return 8;
  return requested;
}

uint32_t pulse_gap_ms(uint32_t step) { return 60u + step * 40u; }
