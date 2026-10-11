//! Deterministic private-fixture generator for the uniform shim ABI.

use std::{fs, io, path::Path};

/// Deliberately-invalid declarations used to prove compiler header checking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureMismatch {
    Width,
    Qualifier,
    Arity,
}

/// Writes the private fixture's uniform-entry translation unit.
///
/// # Errors
///
/// Returns an error when the output cannot be written.
pub fn generate(output: &Path, mismatch: Option<FixtureMismatch>) -> io::Result<()> {
    fs::write(output, source(mismatch))
}

fn source(mismatch: Option<FixtureMismatch>) -> String {
    let check = match mismatch {
        None => {
            r"
static slug_spike_handle *(*const check_open_read)(const char *) = slug_spike_open_read;
static slug_spike_handle *(*const check_open_write)(const char *) = slug_spike_open_write;
static slug_spike_handle *(*const check_open_append)(const char *) = slug_spike_open_append;
static slug_spike_status (*const check_read_line)(slug_spike_handle *, const char **) = slug_spike_read_line;
static int64_t (*const check_write)(slug_spike_handle *, const char *, uint64_t) = slug_spike_write;
static slug_spike_status (*const check_close)(slug_spike_handle *) = slug_spike_close;
static int64_t (*const check_add)(int64_t, int64_t) = slug_spike_add;
"
        }
        Some(FixtureMismatch::Width) => {
            "static int (*const check_add)(int, int) = slug_spike_add;\n"
        }
        Some(FixtureMismatch::Qualifier) => {
            "static slug_spike_handle *(*const check_open_read)(char *) = slug_spike_open_read;\n"
        }
        Some(FixtureMismatch::Arity) => {
            "static int64_t (*const check_add)(int64_t) = slug_spike_add;\n"
        }
    };
    format!(
        r#"#include "typed_shim_abi.h"
#include "typed_dispatch_fixture.h"
{check}
static int32_t open_read(const slug_value *a, slug_value *r, slug_shim_cleanup *c) {{
  (void)c; r[0].as.pointer = check_open_read(a[0].as.pointer); r[0].length = 0; return r[0].as.pointer ? 0 : 2;
}}
static int32_t open_write(const slug_value *a, slug_value *r, slug_shim_cleanup *c) {{
  (void)c; r[0].as.pointer = check_open_write(a[0].as.pointer); r[0].length = 0; return r[0].as.pointer ? 0 : 2;
}}
static int32_t open_append(const slug_value *a, slug_value *r, slug_shim_cleanup *c) {{
  (void)c; r[0].as.pointer = check_open_append(a[0].as.pointer); r[0].length = 0; return r[0].as.pointer ? 0 : 2;
}}
static int32_t read_line(const slug_value *a, slug_value *r, slug_shim_cleanup *c) {{
  const char *text = 0; slug_spike_status status; (void)c;
  status = check_read_line((slug_spike_handle *)a[0].as.pointer, &text);
  if (status == SLUG_SPIKE_EOF) {{ r[0].as.pointer = 0; r[0].length = 0; return 0; }}
  if (status != SLUG_SPIKE_OK) return (int32_t)status;
  r[0].as.pointer = text; r[0].length = 0; return 0;
}}
static int32_t write(const slug_value *a, slug_value *r, slug_shim_cleanup *c) {{
  int64_t written; (void)c;
  written = check_write((slug_spike_handle *)a[0].as.pointer, a[1].as.pointer, a[1].length);
  if (written < 0) return 2;
  r[0].as.i64 = written; r[0].length = 0; return 0;
}}
static int32_t close_handle(const slug_value *a, slug_value *r, slug_shim_cleanup *c) {{
  (void)r; (void)c; return (int32_t)check_close((slug_spike_handle *)a[0].as.pointer);
}}
static int32_t add(const slug_value *a, slug_value *r, slug_shim_cleanup *c) {{
  (void)c; r[0].as.i64 = check_add(a[0].as.i64, a[1].as.i64); r[0].length = 0; return 0;
}}
static const slug_shim_binding BINDINGS[] = {{
  {{"slug.spike", "openRead", open_read, 1, 1, 0x1001ULL}},
  {{"slug.spike", "openWrite", open_write, 1, 1, 0x1002ULL}},
  {{"slug.spike", "openAppend", open_append, 1, 1, 0x1003ULL}},
  {{"slug.spike", "readLine", read_line, 1, 1, 0x1004ULL}},
  {{"slug.spike", "write", write, 2, 1, 0x1005ULL}},
  {{"slug.spike", "close", close_handle, 1, 0, 0x1006ULL}},
  {{"slug.spike", "add", add, 2, 1, 0x1007ULL}},
}};
static const slug_shim_table TABLE = {{SLUG_SHIM_ABI_VERSION, 7, BINDINGS, 0xfeedULL}};
SLUG_TYPED_DISPATCH_FIXTURE_EXPORT const slug_shim_table *slug_shim_get_table(void) {{ return &TABLE; }}
"#
    )
}
