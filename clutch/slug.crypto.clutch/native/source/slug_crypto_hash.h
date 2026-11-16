#ifndef SLUG_CRYPTO_HASH_H
#define SLUG_CRYPTO_HASH_H

#include <stddef.h>
#include <stdint.h>

/* An incremental SHA-256 (FIPS 180-4): its state, the bytes waiting for a
 * full block, and, once finished, the digest. */
typedef struct slug_hash_sha256 slug_hash_sha256;

/* Status results. */
#define SLUG_HASH_OK 0
#define SLUG_HASH_FINISHED 1

/* A new hash of no bytes yet, or NULL when memory runs out. */
slug_hash_sha256 *slug_hash_sha256_new(void);

/* Adds `length` bytes. Returns SLUG_HASH_OK, or SLUG_HASH_FINISHED after
 * slug_hash_sha256_finish, adding nothing. */
int32_t slug_hash_sha256_update(slug_hash_sha256 *hash, const uint8_t *data, size_t length);

/* As slug_hash_sha256_update, for text as UTF-8. */
int32_t slug_hash_sha256_update_text(slug_hash_sha256 *hash, const char *text, size_t length);

/* Finishes the hash and writes its 32-byte digest, which stays in `hash` until
 * it is released; the host copies it at once. Returns SLUG_HASH_OK, or
 * SLUG_HASH_FINISHED when it was already finished, writing nothing. */
int32_t slug_hash_sha256_finish(slug_hash_sha256 *hash, const uint8_t **digest, size_t *length);

/* Frees `hash`. */
void slug_hash_sha256_release(slug_hash_sha256 *hash);

/* Writes the 32-byte digest of `length` bytes, in a per-thread buffer until
 * the thread's next call; the host copies it at once. */
void slug_hash_sha256_of(const uint8_t *data, size_t length, const uint8_t **digest,
                         size_t *digest_length);

#endif
