use super::*;

#[test]
#[allow(clippy::too_many_lines)]
fn accepts_annotations_and_checks_provable_mismatches_on_request() {
    let path = fixture_path("type-annotations");
    fs::write(
        &path,
        "val label:str|nil = \"ready\"\nval User = struct { name:str = \"Slug\" }\nval double = fn<T>(value:num):num { value * 2 }\nprintln(label, double(2), User {}.name)\n",
    )
    .expect("write annotated source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run annotated source");
    fs::remove_file(&path).expect("remove annotated source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "ready 4 Slug\n");

    fs::write(
        &path,
        "val first = fn<T>(left:T, right:T):T { left }\nprintln(first<str>(\"left\", \"right\"))\n",
    )
    .expect("write generic call source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run generic call source");
    fs::remove_file(&path).expect("remove generic call source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "left\n");

    fs::write(
        &path,
        "val first = fn<T>(left:T, right:T):T { left }\nfirst(1, \"wrong\")\n",
    )
    .expect("write inconsistent generic call");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run inconsistent generic call");
    fs::remove_file(&path).expect("remove inconsistent generic call");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected num, got str")
    );

    fs::write(
        &path,
        "val first = fn<T>(left:T, right:T):T { left }\nfirst(1, \"wrong\") /> println\n",
    )
    .expect("write piped inconsistent generic call");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run piped inconsistent generic call");
    fs::remove_file(&path).expect("remove piped inconsistent generic call");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected num, got str")
    );

    fs::write(&path, "val label:str = 1\n").expect("write mismatched declaration");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run mismatched declaration");
    fs::remove_file(&path).expect("remove mismatched declaration");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got num")
    );

    fs::write(&path, "val label = fn():str { 1 }\n").expect("write mismatched return");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run mismatched return");
    fs::remove_file(&path).expect("remove mismatched return");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got num")
    );

    fs::write(&path, "val User = struct { name:str = 1 }\n")
        .expect("write mismatched struct default");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run mismatched struct default");
    fs::remove_file(path).expect("remove mismatched struct default");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got num")
    );
}

#[test]
fn infers_precise_function_values_and_return_results() {
    let path = fixture_path("function-value-inference");
    fs::write(
        &path,
        "val increment:fn<num, num> = fn(value:num) { value + 1 }\n\
         val callbacks:list<fn<num, num> > = [increment, fn(value:num) { value * 2 }]\n\
         val answer:num = increment(41)\n\
         println(answer, callbacks)\n",
    )
    .expect("write precise function value source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run precise function value source");
    fs::remove_file(&path).expect("remove precise function value source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "42 [<fn>, <fn>]\n"
    );

    for (_kind, source, expected) in [
        (
            "function-value-parameter-mismatch",
            "val invalid:fn<str, num> = fn(value:str):str { value }\n",
            "expected fn<str, num>, got fn<str, str>",
        ),
        (
            "inferred-function-return-mismatch",
            "val number = fn() { 1 }\nval invalid:str = number()\n",
            "expected str, got num",
        ),
    ] {
        fs::write(&path, source).expect("write invalid function value source");
        let output = slug()
            .arg("-type-check")
            .arg(&path)
            .output()
            .expect("run invalid function value source");
        fs::remove_file(&path).expect("remove invalid function value source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.starts_with(&format!("slug: semantic error: {expected}")),
            "{stderr}"
        );
    }
}

#[test]
fn checks_positional_calls_through_precise_function_values() {
    let path = fixture_path("function-value-calls");
    fs::write(
        &path,
        "val ready = true\n\
         val choose:fn<str, str> = if (ready) {\n\
           fn(value:str):str { value }\n\
         } else {\n\
           fn(value:str):str { \"fallback\" }\n\
         }\n\
         val result:str = choose(\"value\")\n\
         println(result)\n",
    )
    .expect("write function value call source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run function value call source");
    fs::remove_file(&path).expect("remove function value call source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "value\n");

    for (source, expected) in [
        (
            "val choose:fn<str, str> = if (true) { fn(value:str):str { value } } else { fn(value:str):str { value } }\nchoose(1)\n",
            "expected str, got num",
        ),
        (
            "val choose:fn<str, str> = if (true) { fn(value:str):str { value } } else { fn(value:str):str { value } }\nchoose()\n",
            "function value expects 1 argument, got 0",
        ),
    ] {
        fs::write(&path, source).expect("write invalid function value call source");
        let output = slug()
            .arg("-type-check")
            .arg(&path)
            .output()
            .expect("run invalid function value call source");
        fs::remove_file(&path).expect("remove function value call source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.starts_with(&format!("slug: semantic error: {expected}")),
            "{stderr}"
        );
    }
}

#[test]
fn enforces_any_nil_and_canonical_type_rules() {
    let path = fixture_path("semantic-types");
    fs::write(
        &path,
        "val nonNil:any = \"ready\"\n\
         val nullable:any|nil = nil\n\
         val source:str|nil = \"value\"\n\
         val duplicate:str|nil = source\n\
         val values:list<str|nil> = [\"value\", nil]\n\
         val same:list<str|nil> = values\n\
         val safe = fn():any { nonNil }\n\
         val maybe = fn():any|nil { nullable }\n\
         println(duplicate, same, safe(), maybe())\n",
    )
    .expect("write canonical semantic type source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run canonical semantic type source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "value [\"value\", nil] ready nil\n"
    );

    fs::write(&path, "val invalid:any = nil\n").expect("write nil-to-any source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run nil-to-any source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: expected any, got nil")
    );

    fs::write(
        &path,
        "val identity = fn<T>(value:T):T { value }\nidentity(nil)\n",
    )
    .expect("write nil generic inference source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run nil generic inference source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: generic type argument cannot include nil")
    );

    fs::write(&path, "val invalid:nmu = 1\n").expect("write unknown annotation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run unknown annotation source");
    fs::remove_file(path).expect("remove semantic type source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: unknown type `nmu`")
    );
}

#[test]
fn resolves_statically_known_calls_through_lexical_callable_scopes() {
    let path = fixture_path("scoped-callables");
    fs::write(
        &path,
        "val render = fn(value:str):str { \"outer:\" + value }\n\
         val alias = render\n\
         val invoke = fn(render) { render(2) }\n\
         val inner = {\n\
           val render = fn(value:num):num { value + 1 }\n\
           render(2)\n\
         }\n\
         println(inner, render(\"ok\"), alias(\"alias\"), invoke(fn(value) { value + 3 }))\n",
    )
    .expect("write scoped callable source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run scoped callable source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "3 outer:ok outer:alias 5\n"
    );

    fs::write(
        &path,
        "val render = fn(value:str):str { value }\nrender(1)\n",
    )
    .expect("write statically invalid call source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run statically invalid call source");
    fs::remove_file(path).expect("remove scoped callable source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: expected str, got num")
    );
}

#[test]
fn accepts_tags_and_evaluates_their_arguments_before_declarations() {
    let path = fixture_path("tags");
    fs::write(
        &path,
        "var observed = 0\n@audit(observed = observed + 1)\nval increment = fn(@unit value) { value + 1 }\nprintln(observed, increment(2))\n",
    )
    .expect("write tagged source");
    let output = slug().arg(&path).output().expect("run tagged source");
    fs::remove_file(&path).expect("remove tagged source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1 3\n");

    fs::write(&path, "@export val value = 1\nprintln(value)\n")
        .expect("write legacy export-tag source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run legacy export-tag source");
    fs::remove_file(&path).expect("remove legacy export-tag source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n");

    let cases = [(
        "tagged-expression",
        "@audit println(1)\n",
        "slug: parse error: documentation blocks and tags must prefix a val, var, or foreign declaration",
    )];
    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid tagged source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid tagged source");
        fs::remove_file(path).expect("remove invalid tagged source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with(expected)
        );
    }
}

#[test]
fn attaches_strict_documentation_blocks_to_top_level_declarations() {
    let path = fixture_path("documentation-blocks");
    fs::write(
        &path,
        "/**\n * Adds one to a value.\n */\n// A comment may intervene.\n@deprecated\nval increment = fn(value) { value + 1 }\nprintln(increment(2))\n",
    )
    .expect("write documented source");
    let output = slug().arg(&path).output().expect("run documented source");
    fs::remove_file(&path).expect("remove documented source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "3\n");

    fs::write(
        &path,
        "/**\n * Module documentation.\n */\n\n/**\n * Fibonacci documentation.\n */\nvar fib = fn(n) match {\n x if x < 2 => x\n x => fib(x - 2) + fib(x - 1)\n}\nprintln(fib(6))\n",
    )
    .expect("write module-documented source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run module-documented source");
    fs::remove_file(&path).expect("remove module-documented source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "8\n");

    let cases = [
        (
            "malformed-documentation-block",
            "/**\n not a documentation line\n */\nval value = 1\n",
            "slug: parse error: every non-empty documentation line must begin with *",
        ),
        (
            "misplaced-documentation-block",
            "/**\n * Documentation\n */\nprintln(1)\n",
            "slug: parse error: documentation blocks and tags must prefix a val, var, or foreign declaration",
        ),
        (
            "nested-documentation-block",
            "val value = fn() {\n /**\n  * Documentation\n  */\n val inner = 1\n inner\n}\n",
            "slug: parse error: documentation blocks are only valid at top level",
        ),
    ];
    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid documented source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid documented source");
        fs::remove_file(path).expect("remove invalid documented source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with(expected)
        );
    }
}

#[test]
fn reports_unregistered_documented_foreign_declarations() {
    let path = fixture_path("documented-foreign-declaration");
    fs::write(
        &path,
        "/**\n * creates a new channel with an optional buffer capacity.\n *\n * An unbuffered channel (capacity 0) blocks the sender until a receiver\n * is ready. A buffered channel allows up to `capacity` messages to be\n * queued before blocking.\n */\nexport foreign chan = fn(capacity:num = 0):chan<any|nil>\n",
    )
    .expect("write documented foreign source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run documented foreign source");
    fs::remove_file(&path).expect("remove documented foreign source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("foreign function `")
            && String::from_utf8_lossy(&output.stderr).contains(".chan` is not registered")
    );
}

#[test]
fn imports_slug_channel_with_its_registered_foreign_bindings() {
    let path = fixture_path("slug-channel-library");
    fs::write(
        &path,
        "val channel = import(\"slug.channel\")\n\
         val inbox = channel.chan(2)\n\
         val returned = inbox /> channel.send(7) /> channel.send(42)\n\
         println(returned == inbox)\n\
         println(channel.recv(inbox))\n\
         println(channel.recv(inbox))\n\
         channel.close(inbox)\n\
         println(channel.recv(inbox))\n",
    )
    .expect("write slug.channel source");
    let output = slug()
        .arg(&path)
        .env("SLUG_HOME", env!("CARGO_MANIFEST_DIR"))
        .env_remove("SLUG_FIXTURE_LIBRARY_ROOT")
        .output()
        .expect("run slug.channel source");
    fs::remove_file(path).expect("remove slug.channel source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\n7\n42\nnil\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn discards_function_parameters_without_introducing_bindings() {
    let path = fixture_path("discard-parameters");
    fs::write(
        &path,
        channel_source("val channel = channel(1)\nprintln(0 /> fn(_) { channel })\nprintln(fn(_, _) { 7 }(1, 2))\n"),
    )
    .expect("write discard parameter source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run discard parameter source");
    fs::remove_file(&path).expect("remove discard parameter source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "<chan>\n7\n");
    assert!(output.stderr.is_empty());
}
