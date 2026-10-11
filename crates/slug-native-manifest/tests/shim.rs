//! Generated shims compiled with the platform C compiler: `cc`, or `clang` on
//! Windows, as in the `ffi-prototype` CI job.

use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

use slug_native_manifest::{DYNAMIC_DEFINE, NativeManifest, SHIM_C_FLAGS, check, generate};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "slug-native-shim-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temporary shim directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).expect("write temporary file");
        path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compiler() -> &'static str {
    if cfg!(windows) { "clang" } else { "cc" }
}

fn compile(directory: &TemporaryDirectory, arguments: &[&str]) -> Output {
    Command::new(compiler())
        .args(SHIM_C_FLAGS)
        .arg("-I")
        .arg(directory.path())
        .args(arguments)
        .current_dir(directory.path())
        .output()
        .expect("start the C compiler")
}

/// A driver's standard output with `\n` line endings: the Windows C runtime
/// writes text-mode `stdout` with `\r\n`.
fn driver_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn assert_compiled(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} must compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Writes the generated shim for `manifest` as `<provider>_shim.c`.
fn write_shim(directory: &TemporaryDirectory, manifest: &str, provider: &str) -> NativeManifest {
    let manifest = NativeManifest::from_clutch_source(manifest).expect("valid manifest");
    let shim = generate(&manifest, provider).expect("generate shim");
    directory.write(&format!("{provider}_shim.c"), &shim);
    manifest
}

const FIXTURE_HEADER: &str = r"
#ifndef FX_H
#define FX_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct fx_counter fx_counter;

int64_t fx_sum(int8_t a, int16_t b, int32_t c, int64_t d);
uint64_t fx_usum(uint8_t a, uint16_t b, uint32_t c, uint64_t d);
double fx_scale(float factor, double value);
bool fx_not(bool value);
uint64_t fx_measure(const char *text, size_t length, const uint8_t *bytes, size_t byte_length);
fx_counter *fx_open(const char *name);
const char *fx_name(fx_counter *counter);
int32_t fx_next(fx_counter *counter, const char **text, int64_t *value);
char *fx_describe(fx_counter *counter);
void fx_free(void *pointer);
void fx_close(fx_counter *counter);
int32_t fx_frees(void);
#endif
";

const FIXTURE_SOURCE: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "fx.h"

struct fx_counter { char name[16]; char line[32]; int64_t count; };
static int32_t frees;

int64_t fx_sum(int8_t a, int16_t b, int32_t c, int64_t d) { return a + b + c + d; }
uint64_t fx_usum(uint8_t a, uint16_t b, uint32_t c, uint64_t d) { return a + b + c + d; }
double fx_scale(float factor, double value) { return (double)factor * value; }
bool fx_not(bool value) { return !value; }
uint64_t fx_measure(const char *text, size_t length, const uint8_t *bytes, size_t byte_length) {
  return (uint64_t)(strncmp(text, "hello", length) == 0 ? length + byte_length + bytes[2] : 0u);
}
fx_counter *fx_open(const char *name) {
  fx_counter *counter;
  if (strcmp(name, "missing") == 0) return NULL;
  counter = calloc(1, sizeof *counter);
  snprintf(counter->name, sizeof counter->name, "%s", name);
  return counter;
}
const char *fx_name(fx_counter *counter) { return counter->name; }
int32_t fx_next(fx_counter *counter, const char **text, int64_t *value) {
  counter->count += 1;
  snprintf(counter->line, sizeof counter->line, "%s-%d", counter->name, (int)counter->count);
  *text = counter->line;
  *value = counter->count;
  return 7;
}
char *fx_describe(fx_counter *counter) {
  char *text = malloc(32);
  snprintf(text, 32, "<%s>", counter->name);
  return text;
}
void fx_free(void *pointer) { frees += 1; free(pointer); }
void fx_close(fx_counter *counter) { free(counter); }
int32_t fx_frees(void) { return frees; }
"#;

const FIXTURE_MANIFEST: &str = r#"
[native]
profile = "core"
headers = ["fx.h"]
sources = ["fixture.c"]

[native.resources.fx.Counter]
type = "fx_counter"
release = { symbol = "fx_close", signature = "(handle<Counter>) -> void" }

[native.functions.fx]
sum = { symbol = "fx_sum", signature = "(i8, i16, i32, i64) -> i64" }
usum = { symbol = "fx_usum", signature = "(u8, u16, u32, u64) -> u64" }
scale = { symbol = "fx_scale", signature = "(f32, f64) -> f64" }
not = { symbol = "fx_not", signature = "(bool) -> bool" }
measure = { symbol = "fx_measure", signature = "(str, bytes) -> u64" }
open = { symbol = "fx_open", signature = "(cstr) -> handle<Counter>" }
name = { symbol = "fx_name", signature = "(handle<Counter>) -> cstr" }
next = { symbol = "fx_next", signature = "(handle<Counter>, out cstr, out i64) -> i32" }
describe = { symbol = "fx_describe", signature = "(handle<Counter>) -> cstr owned(fx_free)" }
frees = { symbol = "fx_frees", signature = "() -> i32" }
"#;

/// Mirrors the generated ABI types, which each shim defines privately.
const DRIVER_PRELUDE: &str = r#"
#include <inttypes.h>
#include <stdio.h>
#include <string.h>

typedef struct slug_value {
  union { int64_t i64; uint64_t u64; double f64; const void *pointer; } as;
  uint64_t length;
} slug_value;
typedef struct slug_shim_cleanup { void (*release)(void *context); void *context; } slug_shim_cleanup;
typedef int32_t (*slug_shim_entry)(const slug_value *, slug_value *, slug_shim_cleanup *);
typedef struct slug_shim_binding {
  const char *module; const char *name; const char *signature;
  slug_shim_entry entry; uint32_t argument_count; uint32_t result_count;
} slug_shim_binding;
typedef struct slug_shim_table {
  uint32_t abi_version; uint32_t binding_count; const slug_shim_binding *bindings;
} slug_shim_table;

static void print_table(const slug_shim_table *table) {
  uint32_t index;
  printf("abi %u\n", table->abi_version);
  for (index = 0; index < table->binding_count; index++) {
    const slug_shim_binding *binding = &table->bindings[index];
    printf("%s|%s|%s|%u|%u\n", binding->module, binding->name, binding->signature,
           binding->argument_count, binding->result_count);
  }
}

static slug_shim_entry find(const slug_shim_table *table, const char *name) {
  uint32_t index;
  for (index = 0; index < table->binding_count; index++) {
    if (strcmp(table->bindings[index].name, name) == 0) return table->bindings[index].entry;
  }
  return NULL;
}
"#;

const FIXTURE_DRIVER: &str = r#"
const slug_shim_table *slug_shim_table_fx(void);

static void print_text(const char *label, const slug_value *value) {
  if (value->as.pointer == NULL) printf("%s nil %" PRIu64 "\n", label, value->length);
  else printf("%s %s %" PRIu64 "\n", label, (const char *)value->as.pointer, value->length);
}

int main(void) {
  const slug_shim_table *table = slug_shim_table_fx();
  slug_value a[4];
  slug_value r[3];
  slug_shim_cleanup cleanup = {NULL, NULL};
  const void *counter;
  print_table(table);

  a[0].as.i64 = -1; a[1].as.i64 = -300; a[2].as.i64 = 70000; a[3].as.i64 = INT64_C(1) << 40;
  find(table, "sum")(a, r, &cleanup);
  printf("sum %" PRId64 "\n", r[0].as.i64);
  a[0].as.u64 = 255; a[1].as.u64 = 65535; a[2].as.u64 = 0; a[3].as.u64 = UINT64_MAX - 65790;
  find(table, "usum")(a, r, &cleanup);
  printf("usum %" PRIu64 "\n", r[0].as.u64);
  a[0].as.f64 = 1.5; a[1].as.f64 = 2.0;
  find(table, "scale")(a, r, &cleanup);
  printf("scale %.2f\n", r[0].as.f64);
  a[0].as.u64 = 1;
  find(table, "not")(a, r, &cleanup);
  printf("not %" PRIu64 "\n", r[0].as.u64);
  {
    static const uint8_t bytes[] = {9, 9, 30};
    a[0].as.pointer = "hello, world"; a[0].length = 5;
    a[1].as.pointer = bytes; a[1].length = 3;
    find(table, "measure")(a, r, &cleanup);
    printf("measure %" PRIu64 "\n", r[0].as.u64);
  }
  a[0].as.pointer = "missing";
  find(table, "open")(a, r, &cleanup);
  printf("missing %s\n", r[0].as.pointer == NULL ? "nil" : "handle");
  a[0].as.pointer = "alpha";
  find(table, "open")(a, r, &cleanup);
  counter = r[0].as.pointer;
  a[0].as.pointer = counter;
  find(table, "name")(a, r, &cleanup);
  print_text("name", &r[0]);
  find(table, "next")(a, r, &cleanup);
  printf("next %" PRId64 "\n", r[0].as.i64);
  print_text("next.text", &r[1]);
  printf("next.value %" PRId64 "\n", r[2].as.i64);
  printf("cleanup %s\n", cleanup.release == NULL ? "none" : "set");
  find(table, "describe")(a, r, &cleanup);
  print_text("describe", &r[0]);
  printf("cleanup %s\n", cleanup.context == (void *)r ? "results" : "other");
  cleanup.release(cleanup.context);
  find(table, "frees")(a, r, &cleanup);
  printf("frees %" PRId64 "\n", r[0].as.i64);
  find(table, "release:Counter")(a, r, &cleanup);
  return 0;
}
"#;

#[test]
fn generated_shim_runs_every_step_one_type_through_the_uniform_entry() {
    let directory = TemporaryDirectory::new();
    directory.write("fx.h", FIXTURE_HEADER);
    directory.write("fixture.c", FIXTURE_SOURCE);
    let manifest = write_shim(&directory, FIXTURE_MANIFEST, "fx");
    directory.write("driver.c", &format!("{DRIVER_PRELUDE}{FIXTURE_DRIVER}"));
    let program = format!("fixture{}", env::consts::EXE_SUFFIX);
    let output = compile(
        &directory,
        &["fixture.c", "fx_shim.c", "driver.c", "-o", &program],
    );
    assert_compiled(&output, "the fixture, its shim, and the driver");

    let output = Command::new(directory.path().join(&program))
        .output()
        .expect("run the shim driver");
    assert!(output.status.success(), "the shim driver must exit cleanly");
    let mut expected = format!("abi {}\n", slug_native_manifest::SHIM_ABI_VERSION);
    for binding in manifest.bindings() {
        let _ = writeln!(
            expected,
            "{}|{}|{}|{}|{}",
            binding.module,
            binding.name,
            binding.canonical_signature(),
            binding.signature.argument_cells(),
            binding.signature.result_cells()
        );
    }
    expected.push_str(
        "sum 1099511697475\n\
         usum 18446744073709551615\n\
         scale 3.00\n\
         not 0\n\
         measure 38\n\
         missing nil\n\
         name alpha 5\n\
         next 7\n\
         next.text alpha-1 7\n\
         next.value 1\n\
         cleanup none\n\
         describe <alpha> 7\n\
         cleanup results\n\
         frees 1\n",
    );
    assert_eq!(driver_stdout(&output), expected);
}

const MISMATCH_HEADER: &str = r"
#include <stdint.h>
#include <stdlib.h>
int64_t mm_add(int64_t left, int64_t right);
void mm_rename(char *path);
char *mm_text(void);
void mm_release(char *text);
#define mm_twice(value) ((value) * 2)
";

fn mismatch_manifest(functions: &str) -> String {
    format!(
        "[native]\nprofile = \"core\"\nheaders = [\"mm.h\"]\n\n[native.functions.mm]\n{functions}"
    )
}

fn compile_mismatch_shim(functions: &str) -> Output {
    let directory = TemporaryDirectory::new();
    directory.write("mm.h", MISMATCH_HEADER);
    write_shim(&directory, &mismatch_manifest(functions), "mm");
    compile(&directory, &["-c", "mm_shim.c", "-o", "mm_shim.o"])
}

#[test]
fn matching_declarations_and_macro_targets_compile() {
    let output = compile_mismatch_shim(
        "add = { symbol = \"mm_add\", signature = \"(i64, i64) -> i64\" }\n\
         text = { symbol = \"mm_text\", signature = \"() -> cstr owned(free)\" }\n\
         twice = { symbol = \"mm_twice\", signature = \"(i64) -> i64\", macro = true }\n",
    );
    assert_compiled(&output, "a shim matching its header");
}

#[test]
fn header_checks_reject_width_qualifier_arity_and_free_function_mismatches() {
    for (mismatch, functions) in [
        (
            "width",
            "add = { symbol = \"mm_add\", signature = \"(i32, i32) -> i32\" }",
        ),
        (
            "qualifier",
            "rename = { symbol = \"mm_rename\", signature = \"(cstr) -> void\" }",
        ),
        (
            "arity",
            "add = { symbol = \"mm_add\", signature = \"(i64) -> i64\" }",
        ),
        (
            "free function",
            "text = { symbol = \"mm_text\", signature = \"() -> cstr owned(mm_release)\" }",
        ),
    ] {
        let output = compile_mismatch_shim(functions);
        assert!(
            !output.status.success(),
            "a {mismatch} mismatch must fail against the header"
        );
    }
}

const LIBC_HEADER: &str = r"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#if defined(_WIN32)
#include <io.h>
#else
#include <unistd.h>
#endif
int64_t lc_write(int64_t value);
";

#[test]
fn bindings_named_like_c_library_functions_compile_with_libc_headers() {
    let directory = TemporaryDirectory::new();
    directory.write("lc.h", LIBC_HEADER);
    write_shim(
        &directory,
        "[native]\nprofile = \"core\"\nheaders = [\"lc.h\"]\n\n[native.functions.lc]\n\
         write = { symbol = \"lc_write\", signature = \"(i64) -> i64\" }\n\
         read = { symbol = \"lc_write\", signature = \"(i64) -> i64\" }\n\
         close = { symbol = \"lc_write\", signature = \"(i64) -> i64\" }\n",
        "lc",
    );
    for define in [None, Some(format!("-D{DYNAMIC_DEFINE}"))] {
        let mut arguments = vec!["-c", "lc_shim.c", "-o", "lc_shim.o"];
        arguments.extend(define.as_deref());
        assert_compiled(
            &compile(&directory, &arguments),
            "a shim binding `write`, `read`, and `close`",
        );
    }
}

const PROVIDER_DRIVER: &str = r#"
const slug_shim_table *slug_shim_table_alpha(void);
const slug_shim_table *slug_shim_table_beta(void);

int main(void) {
  slug_value a[2];
  slug_value r[1];
  slug_shim_cleanup cleanup = {NULL, NULL};
  print_table(slug_shim_table_alpha());
  print_table(slug_shim_table_beta());
  a[0].as.i64 = 40; a[1].as.i64 = 2;
  find(slug_shim_table_alpha(), "add")(a, r, &cleanup);
  printf("alpha %" PRId64 "\n", r[0].as.i64);
  find(slug_shim_table_beta(), "add")(a, r, &cleanup);
  printf("beta %" PRId64 "\n", r[0].as.i64);
  return 0;
}
"#;

#[test]
fn two_providers_link_into_one_image() {
    let directory = TemporaryDirectory::new();
    directory.write(
        "pair.h",
        "#include <stdint.h>\nint64_t alpha_add(int64_t a, int64_t b);\nint64_t beta_add(int64_t a, int64_t b);\n",
    );
    directory.write(
        "pair.c",
        "#include \"pair.h\"\n\
         int64_t alpha_add(int64_t a, int64_t b) { return a + b; }\n\
         int64_t beta_add(int64_t a, int64_t b) { return a - b; }\n",
    );
    for provider in ["alpha", "beta"] {
        write_shim(
            &directory,
            &format!(
                "[native]\nprofile = \"core\"\nheaders = [\"pair.h\"]\n\n[native.functions.{provider}]\n\
                 add = {{ symbol = \"{provider}_add\", signature = \"(i64, i64) -> i64\" }}\n"
            ),
            provider,
        );
    }
    directory.write("driver.c", &format!("{DRIVER_PRELUDE}{PROVIDER_DRIVER}"));
    let program = format!("providers{}", env::consts::EXE_SUFFIX);
    let output = compile(
        &directory,
        &[
            "pair.c",
            "alpha_shim.c",
            "beta_shim.c",
            "driver.c",
            "-o",
            &program,
        ],
    );
    assert_compiled(&output, "two shims statically linked into one image");
    let output = Command::new(directory.path().join(&program))
        .output()
        .expect("run the two-provider driver");
    assert!(
        output.status.success(),
        "the two-provider driver must exit cleanly"
    );
    assert_eq!(
        driver_stdout(&output),
        "abi 1\n\
         alpha|add|slug-shim/1;alpha;add;alpha_add;(i64,i64)->i64|2|1\n\
         abi 1\n\
         beta|add|slug-shim/1;beta;add;beta_add;(i64,i64)->i64|2|1\n\
         alpha 42\n\
         beta 38\n"
    );
}

#[test]
fn generation_is_deterministic_and_check_mode_detects_stale_shims() {
    let manifest = NativeManifest::from_clutch_source(FIXTURE_MANIFEST).expect("valid manifest");
    let shim = generate(&manifest, "fx").expect("generate shim");
    assert_eq!(generate(&manifest, "fx").expect("generate shim"), shim);
    check(&manifest, "fx", &shim).expect("a fresh shim passes check");

    let changed = NativeManifest::from_clutch_source(
        &FIXTURE_MANIFEST.replace("(i8, i16, i32, i64) -> i64", "(i8, i16, i32, i32) -> i64"),
    )
    .expect("valid manifest");
    let error = check(&changed, "fx", &shim).expect_err("a stale shim fails check");
    assert!(error.location.starts_with("shim line "), "{error}");
    assert!(error.message.contains("(i8,i16,i32,i32)->i64"), "{error}");

    let error =
        check(&manifest, "fx", &shim[..shim.len() / 2]).expect_err("a truncated shim fails");
    assert!(error.message.contains("stale shim"), "{error}");

    let error = generate(&manifest, "not-an-identifier").expect_err("invalid provider");
    assert_eq!(
        error.to_string(),
        "provider: `not-an-identifier` is not a C identifier"
    );
}

#[test]
fn the_shim_binary_generates_and_checks() {
    let directory = TemporaryDirectory::new();
    let manifest = directory.write("clutch.toml", FIXTURE_MANIFEST);
    let shim = directory.path().join("fx_shim.c");
    let run = |mode: &str| {
        Command::new(env!("CARGO_BIN_EXE_slug-native-shim"))
            .arg(mode)
            .arg(&manifest)
            .arg("fx")
            .arg(&shim)
            .output()
            .expect("run slug-native-shim")
    };
    assert!(run("generate").status.success());
    assert!(run("check").status.success());

    fs::write(&manifest, FIXTURE_MANIFEST.replace("fx_frees", "fx_frees2")).expect("edit manifest");
    let output = run("check");
    assert!(!output.status.success(), "a stale shim must fail check");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stale shim"), "{stderr}");

    let output = run("unknown");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}

#[test]
fn dynamic_builds_reject_missing_platforms_escaping_paths_and_compiler_errors() {
    let directory = TemporaryDirectory::new();
    let manifest = |libraries: &str, sources: &str| {
        NativeManifest::from_clutch_source(&format!(
            "[native]\nprofile = \"core\"\nheaders = [\"bad.h\"]\nsources = [{sources}]\n\
             [native.libraries]\n{libraries}\n\
             [native.functions.bad]\nf = {{ symbol = \"bad_f\", signature = \"() -> i32\" }}\n"
        ))
        .expect("valid manifest")
    };
    let error = slug_native_manifest::build_dynamic(
        directory.path(),
        &manifest("\"linux-x86_64\" = \"lib.so\"", ""),
        "bad",
        "plan9-mips",
    )
    .expect_err("missing platform");
    assert_eq!(
        error.to_string(),
        "native.libraries: no library for platform `plan9-mips`"
    );

    let error = slug_native_manifest::build_dynamic(
        directory.path(),
        &manifest("\"linux-x86_64\" = \"../lib.so\"", ""),
        "bad",
        "linux-x86_64",
    )
    .expect_err("escaping library");
    assert!(
        error
            .to_string()
            .contains("must be a relative path inside the clutch")
    );

    directory.write("bad.h", "#include <stdint.h>\nint64_t bad_f(void);\n");
    let error = slug_native_manifest::build_dynamic(
        directory.path(),
        &manifest("\"linux-x86_64\" = \"out/lib.so\"", ""),
        "bad",
        "linux-x86_64",
    )
    .expect_err("header mismatch");
    assert!(
        error.to_string().starts_with("shim: C compiler failed for"),
        "{error}"
    );
    assert!(directory.path().join("out/lib.shim.c").is_file());
}
