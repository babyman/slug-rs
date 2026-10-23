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
fn a_line_beginning_with_a_prefix_operator_starts_a_statement() {
    let path = fixture_path("prefix-line-start");
    // `-k` and `!v` touching their operand start a statement; `- b` with a
    // space after the `-`, and `!=`, continue the line before.
    fs::write(
        &path,
        "val f = fn(x) { x * 2 }\n\
         val k = f(3)\n\
         -k + 1\n\
         val negate = fn(v) {\n\
           println(\"negating\")\n\
           -v\n\
         }\n\
         val invert = fn(v) {\n\
           println(\"inverting\")\n\
           !v\n\
         }\n\
         val a = 10\n\
         val c = a\n\
           - 3\n\
         val d = a\n\
           != 3\n\
         println(k, negate(4), invert(false), c, d)\n",
    )
    .expect("write prefix line start source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run prefix line start source");
    fs::remove_file(path).expect("remove prefix line start source");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "negating\ninverting\n6 -4 true 7 true\n"
    );
}

#[test]
fn a_line_ending_in_type_arguments_ends_the_statement() {
    let path = fixture_path("type-argument-line-end");
    fs::write(
        &path,
        "val Pair = struct { a, b }\n\
         type Pairs = list<struct<Pair>>\n\
         type Table = map<str, list<num>>\n\
         type Grid = list<list<num>>\n\
         val ready:chan<num> = chan(1)\n\
         val wide = 3 >\n  2\n\
         println(wide)\n",
    )
    .expect("write type argument source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run type argument source");
    fs::remove_file(path).expect("remove type argument source");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "true\n");
}

#[test]
fn an_unknown_name_is_a_compile_error_with_suggestions() {
    let path = fixture_path("unknown-names");
    let run = |source: &str, json: bool| {
        fs::write(&path, source).expect("write unknown name source");
        let mut command = slug();
        if json {
            command.arg("--diagnostic-format=json");
        }
        let output = command
            .arg(&path)
            .output()
            .expect("run unknown name source");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    // A misspelled builtin, local, or global is reported before anything
    // runs, with the names it most likely meant.
    for (source, message) in [
        (
            "println(\"first\")\npirntln(\"hi\")\n",
            "semantic error: unknown name `pirntln`; did you mean `println`?",
        ),
        (
            "val count = 3\nval f = fn() { cuont + 1 }\n",
            "semantic error: unknown name `cuont`; did you mean `count`?",
        ),
        (
            "val f = fn(total) { val step = 1\ntotl + step }\n",
            "semantic error: unknown name `totl`; did you mean `total`?",
        ),
        ("missing\n", "semantic error: unknown name `missing`\n"),
    ] {
        let (code, stdout, stderr) = run(source, false);
        assert_eq!(code, Some(1), "{source}");
        assert!(stdout.is_empty(), "{source}: {stdout}");
        assert!(stderr.contains(message), "{source}: {stderr}");
    }
    let (_, _, stderr) = run("pirntln(\"hi\")\n", true);
    let diagnostic: serde_json::Value =
        serde_json::from_str(stderr.trim()).expect("JSON diagnostic");
    assert_eq!(diagnostic["code"], "unknown_name");
    assert_eq!(diagnostic["suggestions"][0]["edits"][0]["text"], "println");

    // Builtins, the VM's own functions, later top-level declarations, and
    // the names a `{*}` declaration selects from a known map are all known;
    // a `{*}` over a map whose entries are known only when it runs leaves
    // other names to be looked up then.
    let (code, stdout, stderr) = run(
        "val e = Error {msg: \"x\"}\n\
         val later = fn() { soon() }\n\
         val soon = fn() { 1 }\n\
         val {*} = {answer: 42}\n\
         println(int(2.5), len([1]), cfg(\"k\", 0), e.msg, later(), answer)\n",
        false,
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "2 1 0 x 1 42\n");
    let (code, stdout, stderr) = run(
        "val pick = fn(entries) { entries }\nval {*} = pick({late: 7})\nprintln(late)\n",
        false,
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "7\n");
    fs::remove_file(&path).expect("remove unknown name source");
}

#[test]
fn an_assignment_that_discards_a_calls_change_is_a_compile_error() {
    let path = fixture_path("lost-update");
    let run = |source: &str| {
        fs::write(&path, source).expect("write lost update source");
        let output = slug().arg(&path).output().expect("run lost update source");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    // `output` is read before `evaluate` runs, and `evaluate` appends to it
    // through `execute`, so the assignment would discard that append.
    for (source, message, location) in [
        (
            "val run = fn() {\n\
             var output = []\n\
             val execute = fn(n) { output = output :+ \"inner\" }\n\
             val evaluate = fn(n) { execute(n); n }\n\
             output = output :+ evaluate(1)\n\
             output\n\
             }\n",
            "this assignment to `output` discards what the call to `evaluate` at 5:20 assigns to it",
            ":5:10\n",
        ),
        (
            "var log = []\nval note = fn(m) { log = log :+ m; m }\nlog = log :+ note(\"a\")\n",
            "this assignment to `log` discards what the call to `note` at 3:14 assigns to it",
            ":3:7\n",
        ),
    ] {
        let (code, _, stderr) = run(source);
        assert_eq!(code, Some(1), "{source}");
        assert!(stderr.contains("code: lost_update"), "{source}: {stderr}");
        assert!(stderr.contains(message), "{source}: {stderr}");
        assert!(stderr.contains(location), "{source}: {stderr}");
    }

    // Reading after the call keeps its change, and a call that reaches the
    // `var` only by calling its declaring function again changes a new one.
    let (code, stdout, stderr) = run("val run = fn() {\n\
         var output = []\n\
         val execute = fn(n) { output = output :+ \"inner\" }\n\
         val evaluate = fn(n) { execute(n); n }\n\
         val value = evaluate(1)\n\
         output = output :+ value\n\
         output\n\
         }\n\
         val block = fn(depth) {\n\
         var xs = []\n\
         val more = fn() { if (len(xs) < 2) { xs = xs :+ statement(depth); recur() } }\n\
         more()\n\
         xs\n\
         }\n\
         val statement = fn(depth) { if (depth > 0) { block(depth - 1) } else { depth } }\n\
         println(run(), block(1))\n");
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "[\"inner\", 1] [[0, 0], [0, 0]]\n");
    fs::remove_file(&path).expect("remove lost update source");
}

#[test]
fn a_pipe_between_match_patterns_offers_a_comma() {
    let path = fixture_path("pattern-pipe");
    fs::write(
        &path,
        "val f = fn(op) {\n  match op {\n    \"+\" | \"-\" => 1\n    _ => 0\n  }\n}\n",
    )
    .expect("write pattern pipe source");
    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(&path)
        .output()
        .expect("run pattern pipe source");
    fs::remove_file(&path).expect("remove pattern pipe source");
    assert_eq!(output.status.code(), Some(1));
    let diagnostic = json_diagnostic(&output.stderr);
    assert_eq!(
        diagnostic["message"],
        "alternative match patterns are separated by `,`, not `|`, as in `\"+\", \"-\" => ...`"
    );
    assert_eq!(diagnostic["fixes"][0]["edits"][0]["text"], ",");
    assert_eq!(
        diagnostic["fixes"][0]["edits"][0]["range"]["start"]["column"],
        9
    );
}

#[test]
fn match_case_separators_and_function_match_bodies_explain_the_syntax() {
    let path = fixture_path("match-case-separators");
    let run = |source: &str| {
        fs::write(&path, source).expect("write match syntax source");
        let output = slug()
            .arg("--diagnostic-format=json")
            .arg(&path)
            .output()
            .expect("run match syntax source");
        assert_eq!(output.status.code(), Some(1), "{source}");
        json_diagnostic(&output.stderr)
    };

    // A `,` between cases can only be a separator, so it is replaced.
    let diagnostic = run("val f = fn(e) { match e { \"n\" => 1, _ => 0 } }\n");
    assert_eq!(
        diagnostic["message"],
        "match cases are separated by a newline or `;`, not `,`"
    );
    assert_eq!(diagnostic["fixes"][0]["edits"][0]["text"], ";");
    assert_eq!(
        diagnostic["fixes"][0]["edits"][0]["range"]["start"]["column"],
        35
    );

    // Without one, a `;` after the case is suggested.
    let diagnostic = run("val f = fn(e) { match e { \"n\" => 1 _ => 0 } }\n");
    assert_eq!(
        diagnostic["message"],
        "match cases are separated by a newline or `;`"
    );
    assert_eq!(diagnostic["suggestions"][0]["edits"][0]["text"], ";");
    assert_eq!(
        diagnostic["suggestions"][0]["edits"][0]["range"]["start"]["column"],
        35
    );

    // A function's `match` body has no subject.
    let diagnostic = run("val f = fn(x) match x { _ => 0 }\n");
    assert_eq!(
        diagnostic["message"],
        "a function's `match` body matches its arguments and takes no subject: write `fn(x) match { ... }`, or `fn(x) { match x { ... } }`"
    );
    fs::remove_file(&path).expect("remove match syntax source");
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
    assert!(
        stderr.ends_with("\n  in main\n  = code: divide_by_zero\n"),
        "{stderr}"
    );
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
    assert!(
        stderr.starts_with("slug: parse error: match cases are separated by a newline or `;`\n")
    );
    assert!(
        stderr.contains(&format!("    --> {}:3:24\n", path.display())),
        "{stderr}"
    );
    assert!(stderr.contains("    1 |     if(acc <= max) {\n"));
    assert!(stderr.contains("    2 |         match [acc % 3, acc % 5] {\n"));
    assert!(stderr.contains("  > 3 |             [0, 0] => \"FizzBuzz\"x\n"));
    assert!(stderr.contains("      |                                 ^ here\n"));
    assert!(
        stderr.ends_with(
            "\n  = code: expected_syntax\n  = found name `x`\n  = suggestion: insert `;`\n      at 3:24: insert `;`\n"
        ),
        "{stderr}"
    );
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

fn json_diagnostic(stderr: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stderr).expect("diagnostic is valid JSON")
}

#[test]
fn json_mode_reports_parse_and_semantic_errors_structurally() {
    let cases = [
        (
            "json-parse",
            "val = 1\n",
            "parse",
            "expected binding name",
            1,
            5,
        ),
        (
            "json-semantic",
            "1 + true\n",
            "semantic",
            "operator `+` does not accept num and bool",
            1,
            3,
        ),
    ];

    for (kind, source, category, message, line, column) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid Slug source");
        let output = slug()
            .arg("--diagnostic-format=json")
            .arg(&path)
            .output()
            .expect("run JSON diagnostic source");
        fs::remove_file(&path).expect("remove invalid Slug source");

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let diagnostic = json_diagnostic(&output.stderr);
        assert_eq!(diagnostic["version"], 2);
        assert_eq!(diagnostic["category"], category);
        assert!(diagnostic.get("kind").is_none());
        assert!(diagnostic["native"].is_null());
        assert_eq!(diagnostic["message"], message);
        assert_eq!(
            diagnostic["location"]["path"],
            path.to_string_lossy().as_ref()
        );
        assert_eq!(diagnostic["location"]["line"], line);
        assert_eq!(diagnostic["location"]["column"], column);
        assert_eq!(diagnostic["frames"], serde_json::json!([]));
        assert!(diagnostic["cause"].is_null());
    }
}

#[test]
fn narrowing_diagnostics_report_the_actual_surviving_type() {
    let path = fixture_path("narrowed-type-diagnostic");
    let source = "val needs_num = fn(value:num):num { value }\n\
                  val invalid = fn(value:str|nil) {\n\
                    if (value != nil) { needs_num(value) }\n\
                  }\n";
    fs::write(&path, source).expect("write narrowed diagnostic source");

    let text_output = slug()
        .arg(&path)
        .output()
        .expect("run text diagnostic source");
    assert_eq!(text_output.status.code(), Some(1));
    assert!(
        String::from_utf8(text_output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected num, got str")
    );

    let json_output = slug()
        .arg("--diagnostic-format=json")
        .arg(&path)
        .output()
        .expect("run JSON diagnostic source");
    fs::remove_file(path).expect("remove narrowed diagnostic source");
    assert_eq!(json_output.status.code(), Some(1));
    let diagnostic = json_diagnostic(&json_output.stderr);
    assert_eq!(diagnostic["category"], "semantic");
    assert_eq!(diagnostic["message"], "expected num, got str");
}

#[test]
fn json_mode_reports_stable_codes_for_every_category() {
    for (name, source, category, code) in [
        ("json-code-parse", "val =\n", "parse", "expected_syntax"),
        (
            "json-code-number",
            "val x = 1__0\n",
            "parse",
            "invalid_number",
        ),
        (
            "json-code-immutable",
            "val x = 1\nx = 2\n",
            "semantic",
            "immutable_assignment",
        ),
        (
            "json-code-type",
            "val x = 1 + \"a\"\n",
            "semantic",
            "type_mismatch",
        ),
        (
            "json-code-unknown",
            "val x:Missing = 1\n",
            "semantic",
            "unknown_type",
        ),
        (
            "json-code-runtime",
            "val zero = 0\nprintln(1 / zero)\n",
            "runtime",
            "divide_by_zero",
        ),
    ] {
        let path = fixture_path(name);
        fs::write(&path, source).expect("write coded source");
        let output = slug()
            .arg("--diagnostic-format=json")
            .arg(&path)
            .output()
            .expect("run coded source");
        fs::remove_file(&path).expect("remove coded source");

        assert_eq!(output.status.code(), Some(1), "{name}");
        let diagnostic = json_diagnostic(&output.stderr);
        assert_eq!(diagnostic["category"], category, "{name}: {diagnostic}");
        assert_eq!(diagnostic["code"], code, "{name}: {diagnostic}");
        if category == "runtime" {
            assert!(diagnostic["range"].is_null(), "{name}: {diagnostic}");
        } else {
            assert!(
                diagnostic["range"]["end"]["column"].is_u64(),
                "{name}: {diagnostic}"
            );
        }
    }

    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(fixture_path("json-code-missing-entry"))
        .output()
        .expect("run missing entry");
    let diagnostic = json_diagnostic(&output.stderr);
    assert_eq!(diagnostic["category"], "io");
    assert_eq!(diagnostic["code"], "entry_read");
}

#[test]
fn json_mode_reports_the_range_of_the_offending_expression() {
    let path = fixture_path("json-range");
    fs::write(&path, "val x = 1 + \"a\"\n").expect("write range source");
    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(&path)
        .output()
        .expect("run range source");
    fs::remove_file(&path).expect("remove range source");

    let diagnostic = json_diagnostic(&output.stderr);
    assert!(diagnostic["expected"].is_null());
    assert!(diagnostic["actual"].is_null());
    assert_eq!(diagnostic["location"]["column"], 11);
    assert_eq!(
        diagnostic["range"],
        serde_json::json!({
            "start": { "line": 1, "column": 9 },
            "end": { "line": 1, "column": 16 },
        })
    );
}

#[test]
fn json_mode_reports_expected_and_actual() {
    let path = fixture_path("json-expected");
    fs::write(&path, "val x:num = \"a\"\n").expect("write expected source");
    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(&path)
        .output()
        .expect("run expected source");
    fs::remove_file(&path).expect("remove expected source");

    let diagnostic = json_diagnostic(&output.stderr);
    assert_eq!(diagnostic["code"], "type_mismatch");
    assert_eq!(diagnostic["expected"], "num");
    assert_eq!(diagnostic["actual"], "str");
}

#[test]
fn json_mode_reports_machine_applicable_fixes() {
    for (name, source, fixes) in [
        (
            "json-fix-insert",
            "val x = [1",
            serde_json::json!([{
                "message": "insert `]`",
                "edits": [{
                    "range": {
                        "start": { "line": 1, "column": 11 },
                        "end": { "line": 1, "column": 11 },
                    },
                    "text": "]",
                }],
            }]),
        ),
        ("json-fix-none", "val x = [1 2]", serde_json::json!([])),
    ] {
        let path = fixture_path(name);
        fs::write(&path, source).expect("write fix source");
        let output = slug()
            .arg("--diagnostic-format=json")
            .arg(&path)
            .output()
            .expect("run fix source");
        fs::remove_file(&path).expect("remove fix source");
        assert_eq!(json_diagnostic(&output.stderr)["fixes"], fixes, "{name}");
    }
}

#[test]
fn json_mode_encodes_thrown_payloads_only_for_thrown_errors() {
    for (name, source, thrown) in [
        (
            "json-thrown-map",
            "throw {code: 7, tags: [\"a\"]}\n",
            Some(serde_json::json!({ "$map": [["code", 7], ["tags", ["a"]]] })),
        ),
        (
            "json-thrown-nil",
            "throw nil\n",
            Some(serde_json::Value::Null),
        ),
        ("json-not-thrown", "val zero = 0\nprintln(1 / zero)\n", None),
    ] {
        let path = fixture_path(name);
        fs::write(&path, source).expect("write thrown source");
        let output = slug()
            .arg("--diagnostic-format=json")
            .arg(&path)
            .output()
            .expect("run JSON thrown source");
        fs::remove_file(&path).expect("remove thrown source");

        assert_eq!(output.status.code(), Some(1));
        let diagnostic = json_diagnostic(&output.stderr);
        assert_eq!(diagnostic.get("thrown").cloned(), thrown, "{name}");
    }
}

#[test]
fn json_mode_reports_runtime_frames_and_module_failures() {
    let runtime_path = fixture_path("json-runtime");
    fs::write(
        &runtime_path,
        "val denominator = 0\nprintln(1 / denominator)\n",
    )
    .expect("write runtime source");
    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(&runtime_path)
        .output()
        .expect("run JSON runtime source");
    fs::remove_file(&runtime_path).expect("remove runtime source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = json_diagnostic(&output.stderr);
    assert_eq!(diagnostic["category"], "runtime");
    assert_eq!(diagnostic["code"], "divide_by_zero");
    assert_eq!(diagnostic["message"], "division by zero");
    assert_eq!(diagnostic["location"]["line"], 2);
    assert_eq!(diagnostic["location"]["column"], 11);
    assert_eq!(diagnostic["frames"][0]["function"], "main");
    assert!(diagnostic["cause"].is_null());

    let module_path = fixture_path("json-module");
    fs::write(&module_path, "import(\"missing.module\")\n").expect("write module source");
    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(&module_path)
        .output()
        .expect("run JSON module source");
    fs::remove_file(&module_path).expect("remove module source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = json_diagnostic(&output.stderr);
    assert_eq!(diagnostic["category"], "module");
    assert_eq!(diagnostic["code"], "module_not_found");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("missing.module")
    );
}

#[test]
fn json_mode_reports_entry_read_failures() {
    let path = fixture_path("missing-json-entry");
    let _ = fs::remove_file(&path);
    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(&path)
        .output()
        .expect("run missing JSON entry");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = json_diagnostic(&output.stderr);
    assert_eq!(diagnostic["category"], "io");
    assert_eq!(diagnostic["code"], "entry_read");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("cannot read")
    );
    assert!(diagnostic["location"].is_null());
}

#[test]
fn import_compile_errors_keep_the_module_diagnostic_as_cause() {
    let root = std::env::temp_dir().join(format!("slug-cli-import-cause-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create import cause root");
    let library = root.join("lib.slug");
    fs::write(&library, "val f = fn(x:num) { x + 1\n").expect("write broken library");
    let main = root.join("main.slug");
    fs::write(&main, "val lib = import(\"lib\")\nprintln(lib.f(1))\n").expect("write importer");

    let json_output = slug()
        .arg("--diagnostic-format=json")
        .arg(&main)
        .output()
        .expect("run JSON import cause source");
    let text_output = slug()
        .arg(&main)
        .output()
        .expect("run text import cause source");
    fs::remove_dir_all(&root).expect("remove import cause root");

    assert_eq!(json_output.status.code(), Some(1));
    let diagnostic = json_diagnostic(&json_output.stderr);
    assert_eq!(diagnostic["category"], "module");
    assert_eq!(diagnostic["code"], "module");
    assert_eq!(diagnostic["location"]["line"], 1);
    let cause = &diagnostic["cause"];
    assert_eq!(cause["category"], "parse");
    assert_eq!(cause["code"], "expected_syntax");
    assert_eq!(cause["expected"], "`}`");
    assert_eq!(cause["location"]["line"], 2);
    assert!(
        cause["location"]["path"]
            .as_str()
            .expect("cause path")
            .ends_with("lib.slug")
    );
    assert_eq!(cause["fixes"][0]["edits"][0]["text"], "}");

    assert_eq!(text_output.status.code(), Some(1));
    let text = String::from_utf8(text_output.stderr).expect("stderr is UTF-8");
    assert!(
        text.contains("caused by: parse error: expected }"),
        "{text}"
    );
    assert!(text.contains("val f = fn(x:num) { x + 1"), "{text}");
}

#[test]
fn missing_modules_suggest_close_names_in_text_and_json() {
    let root = std::env::temp_dir().join(format!("slug-cli-module-suggest-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create module suggestion root");
    fs::write(root.join("utils.slug"), "export val one = 1\n").expect("write sibling module");
    let main = root.join("main.slug");
    fs::write(&main, "val u = import(\"utlis\")\nprintln(u.one)\n").expect("write importer");

    let json_output = slug()
        .arg("--diagnostic-format=json")
        .arg(&main)
        .output()
        .expect("run JSON module suggestion source");
    let text_output = slug()
        .arg(&main)
        .output()
        .expect("run text module suggestion source");
    fs::remove_dir_all(&root).expect("remove module suggestion root");

    let diagnostic = json_diagnostic(&json_output.stderr);
    assert_eq!(diagnostic["code"], "module_not_found");
    assert_eq!(diagnostic["location"]["column"], 16);
    assert_eq!(diagnostic["range"]["start"]["column"], 16);
    assert_eq!(diagnostic["range"]["end"]["column"], 23);
    assert_eq!(diagnostic["fixes"], serde_json::json!([]));
    let suggestion = &diagnostic["suggestions"][0];
    assert_eq!(suggestion["message"], "did you mean `utils`?");
    assert_eq!(suggestion["edits"][0]["text"], "\"utils\"");

    let text = String::from_utf8(text_output.stderr).expect("stderr is UTF-8");
    assert!(
        text.starts_with("slug: runtime error: module `utlis` was not found\n"),
        "{text}"
    );
    assert!(text.contains(":1:16\n"), "{text}");
    assert!(
        text.contains("      |                ^^^^^^^ here\n"),
        "{text}"
    );
    assert!(text.contains("\n  = code: module_not_found\n"), "{text}");
    assert!(
        text.contains("\n  = suggestion: did you mean `utils`?\n      at 1:16-1:23: replace with `\"utils\"`\n"),
        "{text}"
    );
}

#[test]
fn out_of_range_integer_literals_offer_a_float() {
    let run = |name: &str, source: &str| {
        let path = fixture_path(name);
        fs::write(&path, source).expect("write integer literal source");
        let output = slug()
            .arg("--diagnostic-format=json")
            .arg(&path)
            .output()
            .expect("run integer literal source");
        fs::remove_file(&path).expect("remove integer literal source");
        json_diagnostic(&output.stderr)
    };

    // 2^64 is exact as a float, so appending `.0` is a fix.
    let exact = run("integer-literal-exact", "println(18446744073709551616)\n");
    assert_eq!(exact["code"], "invalid_number");
    assert_eq!(
        exact["message"],
        "integer literal is out of range; the largest is 9223372036854775807"
    );
    assert_eq!(exact["range"]["end"]["column"], 29);
    assert_eq!(exact["fixes"][0]["edits"][0]["text"], ".0");
    assert_eq!(
        exact["fixes"][0]["edits"][0]["range"]["start"]["column"],
        29
    );
    assert_eq!(exact["suggestions"], serde_json::json!([]));

    // This one rounds, so it is only a suggestion.
    let rounded = run("integer-literal-rounded", "println(18014398509481986000)\n");
    assert_eq!(rounded["fixes"], serde_json::json!([]));
    assert_eq!(
        rounded["suggestions"][0]["message"],
        "write it as a float, which rounds it to 18014398509481986048"
    );
    assert_eq!(rounded["suggestions"][0]["edits"][0]["text"], ".0");
}

#[test]
fn reserved_words_used_as_names_say_so() {
    for (name, source, column, word) in [
        ("reserved-binding", "val copy = 1\n", 5, "copy"),
        (
            "reserved-parameter",
            "val f = fn(match) { 1 }\n",
            12,
            "match",
        ),
        (
            "reserved-pattern-shorthand",
            "val m = {a: 1}\nval r = match m { {nil} => 1\n _ => 0 }\n",
            20,
            "nil",
        ),
    ] {
        let path = fixture_path(name);
        fs::write(&path, source).expect("write reserved word source");
        let output = slug()
            .arg("--diagnostic-format=json")
            .arg(&path)
            .output()
            .expect("run reserved word source");
        fs::remove_file(&path).expect("remove reserved word source");
        let diagnostic = json_diagnostic(&output.stderr);
        assert_eq!(diagnostic["code"], "expected_syntax");
        assert_eq!(
            diagnostic["message"],
            format!("`{word}` is a reserved word and cannot be used as a name")
        );
        assert_eq!(diagnostic["actual"], format!("`{word}`"));
        assert_eq!(diagnostic["range"]["start"]["column"], column);
    }
}

#[test]
fn a_module_above_the_search_is_named_without_an_edit() {
    let root = std::env::temp_dir().join(format!("slug-cli-module-above-{}", std::process::id()));
    fs::create_dir_all(root.join("tests")).expect("create module above root");
    fs::write(root.join("jobs.slug"), "export val one = 1\n").expect("write module above");
    let main = root.join("tests").join("basic.slug");
    fs::write(&main, "val j = import(\"jobs\")\nprintln(j.one)\n").expect("write importer");
    let output = slug()
        .arg("--diagnostic-format=json")
        .arg(&main)
        .output()
        .expect("run module above source");
    fs::remove_dir_all(&root).expect("remove module above root");

    let diagnostic = json_diagnostic(&output.stderr);
    assert_eq!(diagnostic["code"], "module_not_found");
    let suggestion = &diagnostic["suggestions"][0];
    assert!(
        suggestion["message"]
            .as_str()
            .is_some_and(|message| message
                .starts_with("`../jobs.slug` exists outside the directories searched")),
        "{suggestion}"
    );
    assert_eq!(suggestion["edits"], serde_json::json!([]));
}
