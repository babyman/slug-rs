#include "slug_crypto_hash.h"

#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

struct slug_hash_sha256 {
  uint32_t state[8];
  /* Bytes added so far, for the length in the final block. */
  uint64_t total;
  uint8_t block[64];
  size_t used;
  bool finished;
  uint8_t digest[32];
};

static const uint32_t ROUND[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u,
    0xab1c5ed5u, 0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu,
    0x9bdc06a7u, 0xc19bf174u, 0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu,
    0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau, 0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u, 0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu,
    0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u, 0xa2bfe8a1u, 0xa81a664bu,
    0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u, 0x19a4c116u,
    0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u,
    0xc67178f2u,
};

static const uint32_t INITIAL[8] = {
    0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
    0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u,
};

static uint32_t rotate(uint32_t value, unsigned bits) {
  return (value >> bits) | (value << (32u - bits));
}

/* Mixes one 64-byte block into the state. */
static void compress(uint32_t state[8], const uint8_t block[64]) {
  uint32_t w[64];
  uint32_t a, b, c, d, e, f, g, h;
  unsigned i;
  for (i = 0; i < 16; i++) {
    w[i] = (uint32_t)block[i * 4] << 24 | (uint32_t)block[i * 4 + 1] << 16 |
           (uint32_t)block[i * 4 + 2] << 8 | (uint32_t)block[i * 4 + 3];
  }
  for (i = 16; i < 64; i++) {
    uint32_t s0 = rotate(w[i - 15], 7) ^ rotate(w[i - 15], 18) ^ (w[i - 15] >> 3);
    uint32_t s1 = rotate(w[i - 2], 17) ^ rotate(w[i - 2], 19) ^ (w[i - 2] >> 10);
    w[i] = w[i - 16] + s0 + w[i - 7] + s1;
  }
  a = state[0];
  b = state[1];
  c = state[2];
  d = state[3];
  e = state[4];
  f = state[5];
  g = state[6];
  h = state[7];
  for (i = 0; i < 64; i++) {
    uint32_t s1 = rotate(e, 6) ^ rotate(e, 11) ^ rotate(e, 25);
    uint32_t choose = (e & f) ^ (~e & g);
    uint32_t t1 = h + s1 + choose + ROUND[i] + w[i];
    uint32_t s0 = rotate(a, 2) ^ rotate(a, 13) ^ rotate(a, 22);
    uint32_t majority = (a & b) ^ (a & c) ^ (b & c);
    uint32_t t2 = s0 + majority;
    h = g;
    g = f;
    f = e;
    e = d + t1;
    d = c;
    c = b;
    b = a;
    a = t1 + t2;
  }
  state[0] += a;
  state[1] += b;
  state[2] += c;
  state[3] += d;
  state[4] += e;
  state[5] += f;
  state[6] += g;
  state[7] += h;
}

static void start(slug_hash_sha256 *hash) {
  memcpy(hash->state, INITIAL, sizeof INITIAL);
  hash->total = 0;
  hash->used = 0;
  hash->finished = false;
}

static void add(slug_hash_sha256 *hash, const uint8_t *data, size_t length) {
  /* Empty input may come as NULL, which memcpy must not see. */
  if (length == 0) return;
  hash->total += (uint64_t)length;
  if (hash->used > 0) {
    size_t room = 64 - hash->used;
    size_t take = length < room ? length : room;
    memcpy(hash->block + hash->used, data, take);
    hash->used += take;
    data += take;
    length -= take;
    if (hash->used < 64) return;
    compress(hash->state, hash->block);
    hash->used = 0;
  }
  while (length >= 64) {
    compress(hash->state, data);
    data += 64;
    length -= 64;
  }
  if (length > 0) {
    memcpy(hash->block, data, length);
    hash->used = length;
  }
}

/* Pads the message with a 1 bit, zeros, and its length in bits, then writes
 * the state big-endian into `digest`. */
static void end(slug_hash_sha256 *hash, uint8_t digest[32]) {
  uint64_t bits = hash->total * 8u;
  unsigned i;
  hash->block[hash->used++] = 0x80;
  if (hash->used > 56) {
    memset(hash->block + hash->used, 0, 64 - hash->used);
    compress(hash->state, hash->block);
    hash->used = 0;
  }
  memset(hash->block + hash->used, 0, 56 - hash->used);
  for (i = 0; i < 8; i++) hash->block[56 + i] = (uint8_t)(bits >> (56 - 8 * i));
  compress(hash->state, hash->block);
  for (i = 0; i < 8; i++) {
    digest[i * 4] = (uint8_t)(hash->state[i] >> 24);
    digest[i * 4 + 1] = (uint8_t)(hash->state[i] >> 16);
    digest[i * 4 + 2] = (uint8_t)(hash->state[i] >> 8);
    digest[i * 4 + 3] = (uint8_t)hash->state[i];
  }
  hash->finished = true;
}

slug_hash_sha256 *slug_hash_sha256_new(void) {
  slug_hash_sha256 *hash = malloc(sizeof *hash);
  if (hash != NULL) start(hash);
  return hash;
}

int32_t slug_hash_sha256_update(slug_hash_sha256 *hash, const uint8_t *data, size_t length) {
  if (hash->finished) return SLUG_HASH_FINISHED;
  add(hash, data, length);
  return SLUG_HASH_OK;
}

int32_t slug_hash_sha256_update_text(slug_hash_sha256 *hash, const char *text, size_t length) {
  return slug_hash_sha256_update(hash, (const uint8_t *)text, length);
}

int32_t slug_hash_sha256_finish(slug_hash_sha256 *hash, const uint8_t **digest, size_t *length) {
  if (hash->finished) return SLUG_HASH_FINISHED;
  end(hash, hash->digest);
  *digest = hash->digest;
  *length = sizeof hash->digest;
  return SLUG_HASH_OK;
}

void slug_hash_sha256_release(slug_hash_sha256 *hash) { free(hash); }

void slug_hash_sha256_of(const uint8_t *data, size_t length, const uint8_t **digest,
                         size_t *digest_length) {
  static _Thread_local uint8_t result[32];
  slug_hash_sha256 hash;
  start(&hash);
  add(&hash, data, length);
  end(&hash, result);
  *digest = result;
  *digest_length = sizeof result;
}
