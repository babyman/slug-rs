# Vendored raylib

`raylib/` is raylib 5.5 (https://github.com/raysan5/raylib, tag `5.5`,
release tarball SHA-256
`aea98ecf5bc5c5e0b789a76de0083a21a70457050ea4cc2aec7566935f5e258e`), under
its zlib license in `raylib/LICENSE`. GLFW, under `raylib/src/external/glfw`,
keeps its own zlib license.

Only what `slug.arcade` builds is kept: `src/` without `rmodels.c`, the
platforms other than `rcore_desktop_glfw.c`, the external headers for
unused file formats, and GLFW's `deps/glad`, `getopt`, and Wayland
protocols. `build-raylib.sh` compiles `rcore`, `rshapes`, `rtextures`,
`rtext`, `utils`, `raudio`, and `rglfw` into a static library; it does not
use raylib's Makefile.

`src/config.h` disables the features whose sources were removed. Each
change is marked `// slug: disabled`.

To update: unpack a new release, copy the same files, reapply the
`config.h` changes, and rebuild with `make stage-native-clutches`.
