use std::{fs, process::Command};

fn slug() -> Command {
    Command::new(env!("CARGO_BIN_EXE_slug"))
}

fn fixture_path(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("slug-cli-{kind}-{}.slug", std::process::id()))
}

#[test]
fn help_describes_the_current_public_capability() {
    let output = slug().arg("--help").output().expect("run slug --help");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains(
        "bindings, functions, blocks, conditionals, match, return, recur, collections, arithmetic and logic, calls, and println"
    ));
    assert!(output.stderr.is_empty());
}

#[test]
fn version_is_available_without_loading_source() {
    let output = slug()
        .arg("--version")
        .output()
        .expect("run slug --version");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("version is UTF-8"),
        "slug-vm 0.1.0\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_a_minimal_calculation_through_the_public_cli() {
    let path = fixture_path("minimal-calculation");
    fs::write(&path, "println(1 + 1)\n").expect("write minimal Slug source");
    let output = slug().arg(&path).output().expect("run minimal Slug source");
    fs::remove_file(path).expect("remove minimal Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "2\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_source_through_the_public_cli() {
    let path = fixture_path("success");
    fs::write(&path, "val total = 6 * 7\nprintln(total)\n").expect("write Slug source");
    let output = slug().arg(&path).output().expect("run Slug source");
    fs::remove_file(path).expect("remove Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_bindings_assignments_comments_and_strings() {
    let path = fixture_path("state");
    fs::write(
        &path,
        "# track mutable state\nvar label = \"Slug\"\nlabel = label + \" VM\"\nprintln(label)\n",
    )
    .expect("write stateful Slug source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run stateful Slug source");
    fs::remove_file(path).expect("remove stateful Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug VM\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_core_functions_blocks_conditionals_and_collections() {
    let path = fixture_path("core-language");
    fs::write(
        &path,
        "val choose = fn(a, b) { if (a > b) { a } else { b } }\n\
         val make = fn(x) { fn(y) { x + y } }\n\
         val total = { val first = 40\n first + 2 }\n\
         val values = [10, 20, 30]\n\
         val key = \"label\"\n\
         val user = {name: \"Slug\", [key]: 7}\n\
         println(choose(2, 9), make(40)(2), total, values[-1], user.name, user[key])\n",
    )
    .expect("write core Slug source");
    let output = slug().arg(&path).output().expect("run core Slug source");
    fs::remove_file(path).expect("remove core Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "9 42 42 30 Slug 7\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn returns_early_from_nested_function_control_flow() {
    let path = fixture_path("explicit-return");
    fs::write(
        &path,
        "val firstPositive = fn(a, b) {\n\
           if (a > 0) { return a }\n\
           if (b > 0) { return b }\n\
           0 - 1\n\
         }\n\
         println(firstPositive(5, 9), firstPositive(-1, 7), firstPositive(-1, -2))\n",
    )
    .expect("write explicit return source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run explicit return source");
    fs::remove_file(path).expect("remove explicit return source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "5 7 -1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn reuses_function_frames_for_tail_recursion() {
    let path = fixture_path("recur");
    fs::write(
        &path,
        "val countTo = fn(n, total) {\n\
           if (n == 0) { total } else { recur(n - 1, total + 1) }\n\
         }\n\
         println(countTo(100_000, 0))\n",
    )
    .expect("write recur source");
    let output = slug().arg(&path).output().expect("run recur source");
    fs::remove_file(path).expect("remove recur source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "100000\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn recur_preserves_values_captured_by_earlier_iterations() {
    let path = fixture_path("recur-capture");
    fs::write(
        &path,
        "val retain = fn(n, saved) {\n\
           val current = n\n\
           if (n == 0) { saved() } else { recur(n - 1, fn() { current }) }\n\
         }\n\
         println(retain(1, fn() { nil }))\n",
    )
    .expect("write recur capture source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recur capture source");
    fs::remove_file(path).expect("remove recur capture source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn matches_literals_and_lists_with_case_local_bindings() {
    let path = fixture_path("match");
    fs::write(
        &path,
        "val describe = fn(value) {\n\
           match value {\n\
             0 => \"zero\"\n\
             [head, ...tail] => head + tail[0]\n\
             _ => \"other\"\n\
           }\n\
         }\n\
         val sum = fn(xs, total) {\n\
           match xs {\n\
             [] => total\n\
             [head, ...tail] => recur(tail, total + head)\n\
           }\n\
         }\n\
         println(describe(0), describe([4, 5]), describe(true), sum([1, 2, 3], 0), match 1 { 0 => \"no\" })\n",
    )
    .expect("write match source");
    let output = slug().arg(&path).output().expect("run match source");
    fs::remove_file(path).expect("remove match source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "zero 9 other 6 nil\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn match_guards_use_case_bindings_and_continue_after_false() {
    let path = fixture_path("match-guards");
    fs::write(
        &path,
        "val classify = fn(value) {\n\
           match value {\n\
             n if n > 0 => \"positive\"\n\
             0 => \"zero\"\n\
             _ => \"negative\"\n\
           }\n\
         }\n\
         val firstLong = fn(value) {\n\
           match value {\n\
             [head, ...tail] if tail[0] > 10 => head\n\
             [head, ...tail] => tail[0]\n\
             _ => nil\n\
           }\n\
         }\n\
         println(classify(3), classify(0), classify(0 - 4), firstLong([1, 5]))\n",
    )
    .expect("write match guard source");
    let output = slug().arg(&path).output().expect("run match guard source");
    fs::remove_file(path).expect("remove match guard source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "positive zero negative 5\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn matches_string_keyed_maps_with_nested_patterns_and_extra_entries() {
    let path = fixture_path("map-patterns");
    fs::write(
        &path,
        "val describe = fn(user) {\n\
           match user {\n\
             {name: \"Slug\"} => \"known\"\n\
             {name, age: years} if years > 17 => name\n\
             _ => \"other\"\n\
           }\n\
         }\n\
         println(describe({name: \"Slug\", extra: true}), describe({name: \"Eve\", age: 20}), describe({name: \"Kid\", age: 5}), describe([]))\n",
    )
    .expect("write map pattern source");
    let output = slug().arg(&path).output().expect("run map pattern source");
    fs::remove_file(path).expect("remove map pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "known Eve other other\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn function_match_bodies_follow_parameter_subject_rules() {
    let path = fixture_path("function-match");
    fs::write(
        &path,
        "val classify = fn(value) match {\n\
           0 => \"zero\"\n\
           n if n > 0 => \"positive\"\n\
           _ => \"negative\"\n\
         }\n\
         val pair = fn(left, right) match {\n\
           [1, 2] => \"one-two\"\n\
           _ => \"other\"\n\
         }\n\
         val empty = fn() match { [] => \"empty\" }\n\
         val sum = fn(xs, total) match {\n\
           [[], total] => total\n\
           [[head, ...tail], total] => recur(tail, total + head)\n\
         }\n\
         println(classify(3), classify(0), classify(0 - 1), pair(1, 2), pair(2, 1), empty(), sum([1, 2, 3], 0))\n",
    )
    .expect("write function match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run function match source");
    fs::remove_file(path).expect("remove function match source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "positive zero negative one-two other empty 6\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn destructures_list_and_map_bindings_with_declared_mutability() {
    let path = fixture_path("destructuring");
    fs::write(
        &path,
        "var [first, ...tail] = [1, 2, 3]\n\
         first = 10\n\
         tail = [7]\n\
         val {name, age: years} = {name: \"Slug\", age: 3, extra: true}\n\
         println(first, tail[0], name, years)\n",
    )
    .expect("write destructuring source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run destructuring source");
    fs::remove_file(path).expect("remove destructuring source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "10 7 Slug 3\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn reports_source_location_for_non_matching_destructuring() {
    let path = fixture_path("destructuring-failure");
    fs::write(&path, "val [head] = []\n").expect("write invalid destructuring source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid destructuring source");
    fs::remove_file(path).expect("remove invalid destructuring source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: destructuring pattern did not match at "));
    assert!(stderr.ends_with(":1:14\n  in main\n"));
}

#[test]
fn rejects_recur_outside_a_function_or_tail_position() {
    let top_level = fixture_path("top-level-recur");
    fs::write(&top_level, "recur()\n").expect("write invalid recur source");
    let output = slug()
        .arg(&top_level)
        .output()
        .expect("run invalid recur source");
    fs::remove_file(top_level).expect("remove invalid recur source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: recur is only valid inside a function")
    );

    let non_tail = fixture_path("non-tail-recur");
    fs::write(&non_tail, "val invalid = fn(n) { recur(n) + 1 }\n")
        .expect("write non-tail recur source");
    let output = slug()
        .arg(&non_tail)
        .output()
        .expect("run non-tail recur source");
    fs::remove_file(non_tail).expect("remove non-tail recur source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: recur is only valid in tail position")
    );

    let wrong_arity = fixture_path("wrong-arity-recur");
    fs::write(&wrong_arity, "val invalid = fn(n) { recur() }\n")
        .expect("write wrong-arity recur source");
    let output = slug()
        .arg(&wrong_arity)
        .output()
        .expect("run wrong-arity recur source");
    fs::remove_file(wrong_arity).expect("remove wrong-arity recur source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: recur expects 1 arguments, got 0")
    );
}

#[test]
fn rejects_top_level_return_with_a_location() {
    let path = fixture_path("top-level-return");
    fs::write(&path, "{\nreturn 1\n}\n").expect("write invalid return source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid return source");
    fs::remove_file(path).expect("remove invalid return source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: semantic error: return is only valid inside a function at "));
    assert!(stderr.ends_with(":2:1\n"));
}

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
        "val user = {[\"name\"]: 1}\n\
         // line comment\n\
         /** documentation comment */\n\
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
}

#[test]
fn rejects_assignment_to_an_immutable_binding_with_a_location() {
    let path = fixture_path("immutable-binding");
    fs::write(&path, "val answer = 1\nanswer = 2\n").expect("write invalid assignment");
    let output = slug().arg(&path).output().expect("run invalid assignment");
    fs::remove_file(path).expect("remove invalid assignment");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: cannot assign to immutable binding `answer` at ")
    );
    assert!(stderr.ends_with(":2:1\n"));
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
    fs::remove_file(path).expect("remove runtime fault source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: division by zero at "));
    assert!(stderr.ends_with(":2:11\n  in main\n"));
}

#[test]
fn reports_source_parse_errors_without_a_host_crash() {
    let path = fixture_path("invalid");
    fs::write(&path, "val = 1\n").expect("write invalid Slug source");
    let output = slug().arg(&path).output().expect("run invalid Slug source");
    fs::remove_file(path).expect("remove invalid Slug source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: parse error: expected binding name at "));
    assert!(stderr.ends_with(":1:5\n"));
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
