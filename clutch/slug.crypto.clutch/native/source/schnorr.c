#include "slug_crypto_schnorr.h"

#include <string.h>

/* libsecp256k1, built here as one translation unit with only the modules
 * BIP-340 verification needs. Its code is not written for the shim's
 * -Wconversion -Wsign-conversion -Werror, so those stay off for it. */
#if defined(__clang__)
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wconversion"
#pragma clang diagnostic ignored "-Wsign-conversion"
#pragma clang diagnostic ignored "-Wunused-function"
#elif defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wconversion"
#pragma GCC diagnostic ignored "-Wsign-conversion"
#pragma GCC diagnostic ignored "-Wunused-function"
#endif
#define ENABLE_MODULE_EXTRAKEYS 1
#define ENABLE_MODULE_SCHNORRSIG 1
#include "../vendor/secp256k1/src/secp256k1.c"
#include "../vendor/secp256k1/src/precomputed_ecmult.c"
#include "../vendor/secp256k1/src/precomputed_ecmult_gen.c"
#if defined(__clang__)
#pragma clang diagnostic pop
#elif defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

bool slug_schnorr_verify(const uint8_t *public_key, size_t public_key_length,
                         const uint8_t *message, size_t message_length,
                         const uint8_t *signature, size_t signature_length) {
  secp256k1_xonly_pubkey key;
  /* The module checks the lengths first and throws; these keep a direct
   * caller from reading past a short buffer. */
  if (public_key_length != SLUG_SCHNORR_PUBLIC_KEY_LENGTH ||
      signature_length != SLUG_SCHNORR_SIGNATURE_LENGTH) {
    return false;
  }
  /* An x coordinate at or above the field size, or with no point on the
   * curve, is no key. */
  if (!secp256k1_xonly_pubkey_parse(secp256k1_context_static, &key, public_key)) return false;
  /* Empty input may come as NULL; the library wants a pointer whenever it
   * reads, and reads nothing for an empty message. */
  if (message == NULL) message = (const uint8_t *)"";
  return secp256k1_schnorrsig_verify(secp256k1_context_static, signature, message,
                                     message_length, &key) == 1;
}
