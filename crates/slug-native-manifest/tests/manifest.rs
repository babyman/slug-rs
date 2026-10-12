use slug_native_manifest::{Direction, MAX_CELLS, NativeManifest, NativeType, Profile};

/// The `slug.io.fs` example from the typed native manifest bindings record.
const FS_MANIFEST: &str = r#"
[modules]
"slug.io.fs" = { source = "modules/fs.slug" }

[native]
profile = "core"
headers = ["native/source/slug_io_fs.h"]
sources = ["native/source/fs.c"]
link = []

[native.libraries]
"macos-aarch64" = "native/macos-aarch64/libslug_io_fs.dylib"

[native.resources."slug.io.fs".File]
type = "slug_fs_file"
release = { symbol = "slug_fs_close", signature = "(handle<File> file) -> void" }

[native.functions."slug.io.fs".openReadRaw]
symbol = "slug_fs_open_read"
signature = "(cstr path) -> handle<File>"

[native.functions."slug.io.fs".readLineRaw]
symbol = "slug_fs_read_line"
signature = "(handle<File> file, out cstr line) -> i32"

[native.functions."slug.io.fs".closeRaw]
symbol = "slug_fs_close"
signature = "(close handle<File> file) -> void"
"#;

fn native(body: &str) -> String {
    format!("[native]\nprofile = \"core\"\nheaders = [\"h.h\"]\n{body}")
}

fn rejection(source: &str) -> (String, String) {
    let error = NativeManifest::from_clutch_source(source).expect_err("manifest must be rejected");
    (error.location, error.message)
}

#[test]
fn parses_the_decision_record_fs_example() {
    let manifest = NativeManifest::from_clutch_source(FS_MANIFEST).expect("valid manifest");

    assert_eq!(manifest.profile, Profile::Core);
    assert_eq!(manifest.headers, ["native/source/slug_io_fs.h"]);
    assert_eq!(manifest.sources, ["native/source/fs.c"]);
    assert!(manifest.link.is_empty());
    assert_eq!(
        manifest.libraries.get("macos-aarch64").map(String::as_str),
        Some("native/macos-aarch64/libslug_io_fs.dylib")
    );
    let names = manifest
        .bindings()
        .map(|binding| binding.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["closeRaw", "openReadRaw", "readLineRaw", "release:File"]
    );

    let canonical = |name: &str| {
        manifest
            .bindings()
            .find(|binding| binding.name == name)
            .expect("binding exists")
            .canonical_signature()
    };
    assert_eq!(
        canonical("readLineRaw"),
        "slug-shim/1;slug.io.fs;readLineRaw;slug_fs_read_line;(handle<File:slug_fs_file>,out cstr)->i32"
    );
    assert_eq!(
        canonical("openReadRaw"),
        "slug-shim/1;slug.io.fs;openReadRaw;slug_fs_open_read;(cstr)->handle<File:slug_fs_file>"
    );
    assert_eq!(
        canonical("closeRaw"),
        "slug-shim/1;slug.io.fs;closeRaw;slug_fs_close;(close handle<File:slug_fs_file>)->void"
    );
    assert_eq!(
        canonical("release:File"),
        "slug-shim/1;slug.io.fs;release:File;slug_fs_close;(handle<File:slug_fs_file>)->void"
    );

    let read_line = &manifest.functions[2].signature;
    assert_eq!(read_line.argument_cells(), 1);
    assert_eq!(read_line.result_cells(), 2);
    assert_eq!(read_line.parameters[1].direction, Direction::Out);
    assert_eq!(read_line.parameters[1].name.as_deref(), Some("line"));
}

#[test]
fn canonical_text_ignores_formatting_names_and_key_order() {
    let compact = native(
        "[native.functions.m.f]\n\
         symbol = \"c_f\"\n\
         signature = \"(i64 left,cstr text)->cstr owned(sqlite3_free)\"\n",
    );
    let spaced = native(
        "# a comment\n\
         [native.functions.m.f]\n\
         signature = \"  ( i64 , cstr )  ->  cstr owned( sqlite3_free )  \"\n\
         symbol = \"c_f\"\n",
    );
    let canonical = |source: &str| {
        NativeManifest::from_clutch_source(source)
            .expect("valid manifest")
            .functions[0]
            .canonical_signature()
    };
    assert_eq!(canonical(&compact), canonical(&spaced));
    assert_eq!(
        canonical(&compact),
        "slug-shim/1;m;f;c_f;(i64,cstr)->cstr owned(sqlite3_free)"
    );
}

#[test]
fn macro_targets_are_marked_in_the_canonical_text() {
    let manifest = NativeManifest::from_clutch_source(&native(
        "[native.functions.m.f]\nsymbol = \"F\"\nsignature = \"(u8) -> bool\"\nmacro = true\n",
    ))
    .expect("valid manifest");
    assert!(manifest.functions[0].is_macro);
    assert_eq!(
        manifest.functions[0].canonical_signature(),
        "slug-shim/1;m;f;F;(u8)->bool;macro"
    );
}

#[test]
fn enforces_the_cell_limit_separately_for_arguments_and_results() {
    let parameters = |count: usize, prefix: &str| vec![format!("{prefix}i32"); count].join(", ");
    let signature = |text: String| {
        native(&format!(
            "[native.functions.m.f]\nsymbol = \"f\"\nsignature = \"{text}\"\n"
        ))
    };

    let at_limit = NativeManifest::from_clutch_source(&signature(format!(
        "({}) -> void",
        parameters(MAX_CELLS, "")
    )))
    .expect("16 argument cells are allowed");
    assert_eq!(at_limit.functions[0].signature.argument_cells(), MAX_CELLS);

    let (_, message) = rejection(&signature(format!(
        "({}) -> void",
        parameters(MAX_CELLS + 1, "")
    )));
    assert_eq!(message, "more than 16 argument cells");

    // The C result counts as a result cell, so 16 `out` values exceed it.
    let (_, message) = rejection(&signature(format!(
        "({}) -> i32",
        parameters(MAX_CELLS, "out ")
    )));
    assert_eq!(message, "more than 16 result cells");
}

#[test]
#[allow(clippy::too_many_lines)] // One table of rejection cases.
fn rejects_invalid_manifests_with_their_location() {
    let function = |signature: &str| {
        native(&format!(
            "[native.functions.m.f]\nsymbol = \"f\"\nsignature = \"{signature}\"\n"
        ))
    };
    let with_resource = |body: &str| {
        native(&format!(
            "[native.resources.m.R]\ntype = \"r_t\"\nrelease = {{ symbol = \"r_free\", signature = \"(handle<R>) -> void\" }}\n{body}"
        ))
    };
    let cases = [
        (
            native("abi = \"slug-ffi-prototype/0.13\"\n"),
            "native",
            "unknown key `abi`",
        ),
        (native("source = \"native/source\"\n"), "native", "unknown key `source`"),
        (
            "[native]\nprofile = \"core\"\nheaders = [\"a\\\\b.h\"]\n".to_owned(),
            "native.headers",
            "cannot be included",
        ),
        (
            "[native]\nprofile = \"core\"\nheaders = [\"/usr/include/a.h\"]\n".to_owned(),
            "native.headers",
            "cannot be included",
        ),
        (
            "[native]\nheaders = [\"h.h\"]\n".to_owned(),
            "native",
            "missing `profile`",
        ),
        (
            "[native]\nprofile = \"threads\"\n".to_owned(),
            "native.profile",
            "unknown profile `threads`",
        ),
        (
            "[native]\nprofile = \"core\"\n[native.functions.m.f]\nsymbol = \"f\"\nsignature = \"() -> void\"\n"
                .to_owned(),
            "native.headers",
            "must name at least one header",
        ),
        (
            native("[native.functions.\"bad module\".f]\nsymbol = \"f\"\nsignature = \"() -> void\"\n"),
            "native.functions",
            "invalid module name `bad module`",
        ),
        (
            native("[native.functions.m.f]\nsymbol = \"f\"\nsignature = \"() -> void\"\nextra = 1\n"),
            "native.functions.\"m\".f",
            "unknown key `extra`",
        ),
        (
            native("[native.functions.m.f]\nsymbol = \"not-c\"\nsignature = \"() -> void\"\n"),
            "native.functions.\"m\".f.symbol",
            "`not-c` is not a C identifier",
        ),
        (
            native("[native.functions.m.f]\nsymbol = \"f\"\nsignature = \"() -> void\"\nmacro = \"yes\"\n"),
            "native.functions.\"m\".f.macro",
            "must be a boolean",
        ),
        (function("(int) -> void"), "native.functions.\"m\".f.signature", "unknown type `int`"),
        (function("(void) -> void"), "native.functions.\"m\".f.signature", "`void` is only valid as a result"),
        (function("(i32) -> void extra"), "native.functions.\"m\".f.signature", "unexpected `extra` after result type"),
        (function("(i32 $) -> void"), "native.functions.\"m\".f.signature", "unexpected character `$`"),
        (function("(i32) -> str"), "native.functions.\"m\".f.signature", "a `str` or `bytes` result must be an `out` parameter"),
        (function("(bytes owned(free)) -> void"), "native.functions.\"m\".f.signature", "only a result can be `owned`"),
        (function("(cstr owned(free)) -> void"), "native.functions.\"m\".f.signature", "only a result can be `owned`"),
        (function("() -> i32 owned(free)"), "native.functions.\"m\".f.signature", "only `cstr`, `str`, and `bytes` can be `owned`, not `i32`"),
        (function("(handle<other.Missing>) -> void"), "native.functions.\"m\".f.signature", "unknown resource `Missing` in module `other`"),
        (function("(close i32) -> void"), "native.functions.\"m\".f.signature", "only a `handle` parameter can be `close`"),
        (function("(handle<Missing>) -> void"), "native.functions.\"m\".f.signature", "unknown resource `Missing` in this module"),
        (
            with_resource("[native.functions.m.f]\nsymbol = \"f\"\nsignature = \"(close handle<R>, close handle<R>) -> void\"\n"),
            "native.functions.\"m\".f.signature",
            "at most one parameter can be `close`",
        ),
        (
            with_resource("[native.functions.other.f]\nsymbol = \"f\"\nsignature = \"(handle<R>) -> void\"\n"),
            "native.functions.\"other\".f.signature",
            "unknown resource `R` in this module",
        ),
        (
            native("[native.resources.m.R]\ntype = \"r_t\"\nrelease = { symbol = \"r_free\", signature = \"(handle<R>, i32) -> void\" }\n"),
            "native.resources.\"m\".R.release.signature",
            "a release takes exactly one `handle<R>` argument",
        ),
        (
            native("[native.resources.m.R]\ntype = \"r_t\"\nrelease = { symbol = \"r_free\", signature = \"(handle<R>) -> cstr\" }\n"),
            "native.resources.\"m\".R.release.signature",
            "a release result is ignored, so it cannot be text",
        ),
        (
            native("[native.resources.m.R]\ntype = \"struct r\"\nrelease = { symbol = \"r_free\", signature = \"(handle<R>) -> void\" }\n"),
            "native.resources.\"m\".R.type",
            "`struct r` is not a C type name",
        ),
        (
            native("[native.resources.m.R]\ntype = \"r_t\"\n"),
            "native.resources.\"m\".R",
            "missing `release`",
        ),
    ];
    for (source, location, message) in cases {
        let (actual_location, actual_message) = rejection(&source);
        assert_eq!(actual_location, location, "{source}");
        assert!(
            actual_message.contains(message),
            "expected `{message}`, got `{actual_message}` for\n{source}"
        );
    }
}

#[test]
fn resolves_handles_to_their_resource_c_type() {
    let manifest = NativeManifest::from_clutch_source(&native(
        "[native.resources.m.Db]\ntype = \"sqlite3\"\nrelease = { symbol = \"sqlite3_close_v2\", signature = \"(handle<Db>) -> i32\" }\n\
         [native.functions.m.open]\nsymbol = \"sqlite3_open\"\nsignature = \"(cstr path, out handle<Db> db) -> i32\"\n",
    ))
    .expect("valid manifest");
    let parameter = &manifest.functions[0].signature.parameters[1];
    assert_eq!(parameter.direction, Direction::Out);
    assert_eq!(
        parameter.native_type,
        NativeType::Handle {
            resource: "Db".into(),
            c_type: "sqlite3".into(),
        }
    );
    assert_eq!(
        manifest.resources[0].release.canonical_signature(),
        "slug-shim/1;m;release:Db;sqlite3_close_v2;(handle<Db:sqlite3>)->i32"
    );
}

#[test]
fn errors_display_their_manifest_location() {
    let error = NativeManifest::from_clutch_source(&native(
        "[native.functions.m.f]\nsymbol = \"f\"\nsignature = \"(int) -> void\"\n",
    ))
    .expect_err("unknown type");
    assert_eq!(
        error.to_string(),
        "native.functions.\"m\".f.signature: unknown type `int`"
    );
}

#[test]
fn length_results_and_shared_handles_have_canonical_text() {
    let manifest = NativeManifest::from_clutch_source(
        r#"
[native]
profile = "core"
headers = ["db.h"]

[native.resources.db.Database]
type = "sqlite3"
release = { symbol = "db_close", signature = "(handle<Database>) -> void" }

[native.resources."db.statement".Statement]
type = "sqlite3_stmt"
release = { symbol = "stmt_close", signature = "(handle<Statement>) -> void" }

[native.functions.db]
text = { symbol = "db_text", signature = "(handle<db.Database> database, out str text, out bytes owned(free) blob) -> i32" }

[native.functions."db.statement"]
prepare = { symbol = "db_prepare", signature = "(handle<db.Database> database, str sql) -> handle<Statement>" }
"#,
    )
    .expect("valid manifest");
    let canonical = |name: &str| {
        manifest
            .bindings()
            .find(|binding| binding.name == name)
            .expect("binding exists")
            .canonical_signature()
    };
    assert_eq!(
        canonical("text"),
        "slug-shim/1;db;text;db_text;(handle<Database:sqlite3>,out str,out bytes owned(free))->i32"
    );
    assert_eq!(
        canonical("prepare"),
        "slug-shim/1;db.statement;prepare;db_prepare;(handle<db.Database:sqlite3>,str)->handle<Statement:sqlite3_stmt>"
    );
    let text = manifest
        .functions
        .iter()
        .find(|binding| binding.name == "text")
        .unwrap();
    assert_eq!(text.signature.argument_cells(), 1);
    assert_eq!(text.signature.result_cells(), 3);
}

#[test]
fn cross_module_handles_match_import_qualified_annotations() {
    use slug_loader::{ForeignDeclaration, ForeignParameter};
    use slug_native_manifest::check_foreign_declarations;

    let manifest = NativeManifest::from_clutch_source(
        r#"
[native]
profile = "core"
headers = ["db.h"]

[native.resources.db.Database]
type = "sqlite3"
release = { symbol = "db_close", signature = "(handle<Database>) -> void" }

[native.functions."db.statement"]
check = { symbol = "db_check", signature = "(handle<db.Database> database) -> i32" }
reopen = { symbol = "db_reopen", signature = "(str path) -> handle<db.Database>" }
text = { symbol = "db_text", signature = "(out str text) -> void" }
"#,
    )
    .expect("valid manifest");
    let declaration = |name: &str, parameter: Option<&str>, result: &str| ForeignDeclaration {
        name: name.into(),
        parameters: parameter
            .map(|annotation| ForeignParameter {
                name: "value".into(),
                annotation: Some(annotation.into()),
                variadic: false,
                has_default: false,
            })
            .into_iter()
            .collect(),
        result: Some(result.into()),
    };
    let valid = [
        declaration("check", Some("sqlite.Database"), "num"),
        declaration("reopen", Some("str"), "nil|sqlite.Database"),
        declaration("text", None, "list"),
    ];
    check_foreign_declarations(&manifest, "db.statement", &valid).expect("compatible");

    let wrong_resource = [
        declaration("check", Some("sqlite.Statement"), "num"),
        valid[1].clone(),
        valid[2].clone(),
    ];
    let error = check_foreign_declarations(&manifest, "db.statement", &wrong_resource)
        .expect_err("a different resource must not match");
    assert!(
        error.contains("parameter `value` is annotated `sqlite.Statement`"),
        "{error}"
    );

    let unqualified = [
        declaration("check", Some("Database"), "num"),
        valid[1].clone(),
        valid[2].clone(),
    ];
    assert!(
        check_foreign_declarations(&manifest, "db.statement", &unqualified).is_err(),
        "another module's resource is named through an import"
    );
}

#[test]
fn producer_arguments_need_the_events_profile_and_match_channels() {
    use slug_loader::{ForeignDeclaration, ForeignParameter};
    use slug_native_manifest::{check_foreign_declarations, uses_producers};

    let source = |profile: &str, signature: &str| {
        format!(
            "[native]\nprofile = \"{profile}\"\nheaders = [\"io.h\"]\n\n\
             [native.functions.io]\nstart = {{ symbol = \"io_start\", signature = \"{signature}\" }}\n"
        )
    };
    let manifest = NativeManifest::from_clutch_source(&source("events", "(producer lines) -> i32"))
        .expect("valid events manifest");
    assert!(uses_producers(&manifest));
    assert_eq!(
        manifest.functions[0].canonical_signature(),
        "slug-shim/1;io;start;io_start;(producer)->i32"
    );
    let declaration = |annotation: &str| ForeignDeclaration {
        name: "start".into(),
        parameters: vec![ForeignParameter {
            name: "lines".into(),
            annotation: Some(annotation.into()),
            variadic: false,
            has_default: false,
        }],
        result: Some("num".into()),
    };
    for annotation in ["chan", "chan<str>"] {
        check_foreign_declarations(&manifest, "io", &[declaration(annotation)])
            .unwrap_or_else(|error| panic!("{annotation}: {error}"));
    }
    assert!(check_foreign_declarations(&manifest, "io", &[declaration("str")]).is_err());

    for (profile, signature, message) in [
        (
            "core",
            "(producer lines) -> i32",
            "needs `profile = \"events\"`",
        ),
        (
            "events",
            "(out producer lines) -> void",
            "a `producer` is an argument only",
        ),
        (
            "events",
            "() -> producer",
            "a `producer` is an argument only",
        ),
    ] {
        let error = NativeManifest::from_clutch_source(&source(profile, signature))
            .expect_err(signature)
            .to_string();
        assert!(error.contains(message), "{signature}: {error}");
    }
}
