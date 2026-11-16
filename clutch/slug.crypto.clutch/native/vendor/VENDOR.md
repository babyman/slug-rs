# Vendored libsecp256k1

`secp256k1/` is bitcoin-core's libsecp256k1 0.8.0
(https://github.com/bitcoin-core/secp256k1, tag `v0.8.0`, commit
`6e2c8bc4ecdc6e71dbe7a368f360d8d453ce435d`), under its MIT license in
`secp256k1/COPYING`. Its files are unchanged.

Only what `slug.crypto.schnorr` builds is kept: `src/secp256k1.c`, the two
precomputed tables, the headers they include with the `extrakeys` and
`schnorrsig` modules, and the 32-bit field, scalar, modular-inverse, and
128-bit-integer headers the library picks on platforms without
`__int128`. Tests, benchmarks, tools, the other modules, and the build
systems are not.

`native/source/schnorr.c` compiles it as one translation unit, enabling
the two modules and turning off the shim's conversion warnings for its code;
it does not use the library's own build.

To update: check out a new release, copy the same files (the compiler's
`-MM` output for `schnorr.c` lists them), and run the clutch's tests after
`make stage-native-clutches`.
