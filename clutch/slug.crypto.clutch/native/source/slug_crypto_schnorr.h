#ifndef SLUG_CRYPTO_SCHNORR_H
#define SLUG_CRYPTO_SCHNORR_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define SLUG_SCHNORR_PUBLIC_KEY_LENGTH 32
#define SLUG_SCHNORR_SIGNATURE_LENGTH 64

/* Whether `signature` is a valid BIP-340 Schnorr signature of `message` by
 * the x-only secp256k1 `public_key`. A key or signature of the wrong length,
 * a key that is not on the curve, and a signature with values out of range
 * are all false. Uses no state, so any thread may call it. */
bool slug_schnorr_verify(const uint8_t *public_key, size_t public_key_length,
                         const uint8_t *message, size_t message_length,
                         const uint8_t *signature, size_t signature_length);

#endif
