use super::*;

#[test]
fn bare_map_keys_and_dot_access_use_strings() {
    let path = fixture_path("string-map-keys");
    fs::write(
        &path,
        "val key = \"name\"\nval user = {name: \"Slug\"}\nprintln(user.name, user[key], user[\"name\"])\n",
    )
    .expect("write string-keyed map source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run string-keyed map source");
    fs::remove_file(path).expect("remove string-keyed map source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug Slug Slug\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_removed_symbol_literals() {
    let path = fixture_path("removed-symbol-literal");
    fs::write(&path, "println(:name)\n").expect("write removed symbol literal source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run removed symbol literal source");
    fs::remove_file(path).expect("remove symbol literal source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected expression")
    );
}

#[test]
fn closures_share_mutable_lexical_bindings() {
    let path = fixture_path("mutable-capture");
    fs::write(
        &path,
        "val makeCounter = fn() {\n\
           var value = 0\n\
           fn() {\n\
             value = value + 1\n\
             value\n\
           }\n\
         }\n\
         val counter = makeCounter()\n\
         val makePair = fn() {\n\
           var value = 0\n\
           val increment = fn() { value = value + 1 }\n\
           val current = fn() { value }\n\
           [increment, current]\n\
         }\n\
         val pair = makePair()\n\
         pair[0]()\n\
         pair[0]()\n\
         println(counter(), counter(), pair[1]())\n",
    )
    .expect("write mutable capture source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run mutable capture source");
    fs::remove_file(path).expect("remove mutable capture source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1 2 2\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn short_circuits_logical_operators_and_continues_across_newlines() {
    let path = fixture_path("logical-operators");
    fs::write(
        &path,
        "var calls = 0\n\
         val bump = fn() {\n\
           calls = calls + 1\n\
           true\n\
         }\n\
         false &&\n\
           bump()\n\
         true\n\
           || bump()\n\
         val both = true &&\n\
           true\n\
         val either = false\n\
           || true\n\
         println(calls, both, either)\n",
    )
    .expect("write logical operator source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run logical operator source");
    fs::remove_file(path).expect("remove logical operator source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "0 true true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn handles_comments_and_multiline_delimited_expressions() {
    let path = fixture_path("newlines");
    fs::write(
        &path,
        "println(1) # comment\n\
         println(2)\n\
         println(1\n\
         - 2)\n\
         println(\n\
           3\n\
         )\n\
         println([\n\
           1,\n\
           2\n\
         ][-1])\n\
         println({ [1, 2] })\n",
    )
    .expect("write multiline source");
    let output = slug().arg(&path).output().expect("run multiline source");
    fs::remove_file(path).expect("remove multiline source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1\n2\n-1\n3\n2\n[1, 2]\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn supports_all_documented_comment_forms_and_dot_string_lookup() {
    let path = fixture_path("comments-and-dot-access");
    fs::write(
        &path,
        "/**\n\
         * Documentation comment\n\
         */\n\
         val user = {[\"name\"]: 1}\n\
         // line comment\n\
         println(user.name) /* block comment */\n",
    )
    .expect("write comment source");
    let output = slug().arg(&path).output().expect("run comment source");
    fs::remove_file(path).expect("remove comment source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn parses_long_prefix_sequences_without_recursion() {
    let path = fixture_path("prefix-depth");
    let source = format!("println({}true)\n", "!".repeat(100_000));
    fs::write(&path, source).expect("write deeply prefixed source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run deeply prefixed source");
    fs::remove_file(path).expect("remove deeply prefixed source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_deeply_nested_source_without_aborting() {
    let path = fixture_path("nesting-depth");
    let source = format!("println({}true{})\n", "(".repeat(600), ")".repeat(600));
    fs::write(&path, source).expect("write deeply nested source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run deeply nested source");
    fs::remove_file(path).expect("remove deeply nested source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("slug: parse error: source nesting limit exceeded")
    );

    let path = fixture_path("at-pattern-nesting-depth");
    let source = format!("match 1 {{ {}_ => 1 }}\n", "value @ ".repeat(600));
    fs::write(&path, source).expect("write deeply nested at pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run deeply nested at pattern source");
    fs::remove_file(path).expect("remove deeply nested at pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("slug: parse error: source nesting limit exceeded")
    );
}

#[test]
fn rejects_assignment_to_an_immutable_binding_with_a_location() {
    let path = fixture_path("immutable-binding");
    fs::write(&path, "val answer = 1\nanswer = 2\n").expect("write invalid assignment");
    let output = slug().arg(&path).output().expect("run invalid assignment");
    fs::remove_file(&path).expect("remove invalid assignment");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: cannot assign to immutable binding `answer`\n")
    );
    assert!(stderr.contains(&format!("    --> {}:2:1\n", path.display())));
}

#[test]
fn retains_source_locations_for_runtime_faults_from_source() {
    let path = fixture_path("runtime-location");
    fs::write(&path, "val denominator = 0\nprintln(1 / denominator)\n")
        .expect("write runtime fault source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run runtime fault source");
    fs::remove_file(&path).expect("remove runtime fault source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: division by zero\n"));
    assert!(stderr.contains(&format!("    --> {}:2:11\n", path.display())));
    assert!(stderr.ends_with("\n  in main\n"), "{stderr}");
}

#[test]
fn reports_source_parse_errors_without_a_host_crash() {
    let path = fixture_path("invalid");
    fs::write(&path, "val = 1\n").expect("write invalid Slug source");
    let output = slug().arg(&path).output().expect("run invalid Slug source");
    fs::remove_file(&path).expect("remove invalid Slug source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: parse error: expected binding name\n"));
    assert!(stderr.contains(&format!("    --> {}:1:5\n", path.display())));
}

#[test]
fn renders_available_source_context_for_parse_errors() {
    let path = fixture_path("parse-error-context");
    fs::write(
        &path,
        "\tif(acc <= max) {\n\t\tmatch [acc % 3, acc % 5] {\n\t\t\t[0, 0] => \"FizzBuzz\"x\n",
    )
    .expect("write invalid match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid match source");
    fs::remove_file(&path).expect("remove invalid match source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: parse error: expected match case separator\n"));
    assert!(
        stderr.contains(&format!("    --> {}:3:24\n", path.display())),
        "{stderr}"
    );
    assert!(stderr.contains("    1 |     if(acc <= max) {\n"));
    assert!(stderr.contains("    2 |         match [acc % 3, acc % 5] {\n"));
    assert!(stderr.contains("  > 3 |             [0, 0] => \"FizzBuzz\"x\n"));
    assert!(stderr.ends_with("      |                                 ^ here\n"));
}

#[test]
fn rejects_malformed_call_and_variadic_parameter_lists_with_locations() {
    let cases = [
        (
            "positional-after-named",
            "println(label = \"Slug\", 1)\n",
            "slug: parse error: positional argument cannot appear after a named argument",
        ),
        (
            "spread-after-named",
            "println(label = \"Slug\", ...[1])\n",
            "slug: parse error: spread argument cannot appear after a named argument",
        ),
        (
            "variadic-not-final",
            "val collect = fn(...rest, value) { value }\n",
            "slug: parse error: variadic parameter must be final",
        ),
        (
            "variadic-default",
            "val collect = fn(...rest = []) { rest }\n",
            "slug: parse error: variadic parameters cannot have defaults",
        ),
    ];

    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid call source");
        let output = slug().arg(&path).output().expect("run invalid call source");
        fs::remove_file(path).expect("remove invalid call source");

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
fn reports_runtime_faults_without_a_host_crash() {
    let path = fixture_path("runtime");
    fs::write(&path, "println(1 / 0)\n").expect("write faulting Slug source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run faulting Slug source");
    fs::remove_file(path).expect("remove faulting Slug source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: runtime error: division by zero")
    );
}
