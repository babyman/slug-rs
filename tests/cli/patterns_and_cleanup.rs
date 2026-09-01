use super::*;

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
fn binds_whole_values_with_nested_at_patterns() {
    let path = fixture_path("at-patterns");
    fs::write(
        &path,
        "val describe = fn(value) match {\n\
           whole @ [head, ...tail] => whole[0] + head + tail[0]\n\
           _ => nil\n\
         }\n\
         val whole @ [first, ...rest] = [4, 5]\n\
         println(describe([1, 2]), whole[0], first, rest[0])\n",
    )
    .expect("write at pattern source");
    let output = slug().arg(&path).output().expect("run at pattern source");
    fs::remove_file(path).expect("remove at pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "4 4 4 5\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn failed_nested_at_patterns_continue_to_later_cases() {
    let path = fixture_path("failed-at-pattern");
    fs::write(
        &path,
        "val inspect = fn(value) match {\n\
           whole @ [1, 3] => whole[0]\n\
           [left, right] => left + right\n\
           _ => nil\n\
         }\n\
         println(inspect([1, 2]))\n",
    )
    .expect("write failing at pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run failing at pattern source");
    fs::remove_file(path).expect("remove failing at pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "3\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn matches_non_binding_case_alternatives_with_guards() {
    let path = fixture_path("match-alternatives");
    fs::write(
        &path,
        "val classify = fn(value) match {\n\
           0, 1 if value == 1 => \"one\"\n\
           0, 1 => \"small\"\n\
           [0], [1] => \"list\"\n\
           _ => \"other\"\n\
         }\n\
         println(classify(1), classify(0), classify([1]), classify(3))\n",
    )
    .expect("write match alternative source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run match alternative source");
    fs::remove_file(path).expect("remove match alternative source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "one small list other\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn pinned_patterns_observe_global_local_and_captured_bindings() {
    let path = fixture_path("pinned-bindings");
    fs::write(
        &path,
        "var expected = 1\n\
         val global_match = fn(value) match { ^expected => true; _ => false }\n\
         val make_matcher = fn(expected) { fn(value) match { ^expected => true; _ => false } }\n\
         val captured_match = make_matcher(2)\n\
         val local_match = fn(expected, value) { match value { ^expected => true; _ => false } }\n\
         println(global_match(1), captured_match(2), local_match(3, 3))\n\
         expected = 2\n\
         println(global_match(1), global_match(2))\n",
    )
    .expect("write pinned binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run pinned binding source");
    fs::remove_file(path).expect("remove pinned binding source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "true true true\nfalse true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn pinned_patterns_work_in_collections_destructuring_and_alternatives() {
    let path = fixture_path("nested-pinned-patterns");
    fs::write(
        &path,
        "val expected = 2\n\
         val [^expected, tail] = [2, 3]\n\
         val {status: ^expected, value} = {status: 2, value: \"ok\"}\n\
         val fallback = match [1, 3] {\n\
           [head, ^expected] => head\n\
           [left, right] => left + right\n\
         }\n\
         val alternative = match 2 { ^expected, 0 => true; _ => false }\n\
         println(tail, value, fallback, alternative)\n",
    )
    .expect("write nested pinned pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nested pinned pattern source");
    fs::remove_file(path).expect("remove nested pinned pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "3 ok 4 true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_unknown_and_malformed_pinned_patterns() {
    let unknown = fixture_path("unknown-pinned-pattern");
    fs::write(&unknown, "match 1 { ^missing => true }\n")
        .expect("write unknown pinned pattern source");
    let output = slug()
        .arg(&unknown)
        .output()
        .expect("run unknown pinned pattern source");
    fs::remove_file(unknown).expect("remove unknown pinned pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: unknown pinned binding `missing`")
    );

    let malformed = fixture_path("malformed-pinned-pattern");
    fs::write(&malformed, "match 1 { ^ => true }\n")
        .expect("write malformed pinned pattern source");
    let output = slug()
        .arg(&malformed)
        .output()
        .expect("run malformed pinned pattern source");
    fs::remove_file(malformed).expect("remove malformed pinned pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected pinned binding name")
    );
}

#[test]
fn rejects_bindings_in_match_alternatives() {
    for (label, source) in [
        ("direct", "match 1 { value, 0 => value }\n"),
        ("nested", "match [1] { [value], [] => value }\n"),
        ("at", "match 1 { whole @ 1, 0 => whole }\n"),
    ] {
        let path = fixture_path(&format!("binding-match-alternative-{label}"));
        fs::write(&path, source).expect("write binding match alternative source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run binding match alternative source");
        fs::remove_file(path).expect("remove binding match alternative source");

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with("slug: semantic error: match alternatives cannot introduce bindings")
        );
    }
}

#[test]
fn rejects_trailing_match_alternatives() {
    let path = fixture_path("trailing-match-alternative");
    fs::write(&path, "match 1 { 0, => \"no\" }\n")
        .expect("write trailing match alternative source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run trailing match alternative source");
    fs::remove_file(path).expect("remove trailing match alternative source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected match pattern")
    );
}

#[test]
fn rejects_duplicate_and_malformed_at_patterns() {
    let duplicate = fixture_path("duplicate-at-pattern");
    fs::write(&duplicate, "match [1] { value @ [value] => value }\n")
        .expect("write duplicate at pattern source");
    let output = slug()
        .arg(&duplicate)
        .output()
        .expect("run duplicate at pattern source");
    fs::remove_file(duplicate).expect("remove duplicate at pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: duplicate match binding `value`")
    );

    let malformed = fixture_path("malformed-at-pattern");
    fs::write(&malformed, "match 1 { value @ => value }\n")
        .expect("write malformed at pattern source");
    let output = slug()
        .arg(&malformed)
        .output()
        .expect("run malformed at pattern source");
    fs::remove_file(malformed).expect("remove malformed at pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected match pattern")
    );
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
fn matches_quoted_string_map_pattern_keys() {
    let path = fixture_path("quoted-map-pattern-key");
    fs::write(
        &path,
        "val describe = fn(value) match {\n\
           {\"k\": 1} => \"map with k == 1\"\n\
           _ => \"other\"\n\
         }\n\
         println(describe({k: 1}), describe({k: 2}))\n",
    )
    .expect("write quoted map pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run quoted map pattern source");
    fs::remove_file(path).expect("remove quoted map pattern source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "map with k == 1 other\n"
    );
}

#[test]
fn parses_quoted_string_map_literals_in_pipelines() {
    let path = fixture_path("quoted-map-literal");
    fs::write(
        &path,
        "val f = fn(value) match {\n\
           {\"k\": \"v\"} => \"map with v\"\n\
           _ => \"other\"\n\
         }\n\
         println({\"k\": \"v\"} /> f)\n",
    )
    .expect("write quoted map literal source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run quoted map literal source");
    fs::remove_file(path).expect("remove quoted map literal source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "map with v\n");
}

#[test]
fn computed_map_pattern_keys_support_expressions_and_lexical_bindings() {
    let path = fixture_path("computed-map-patterns");
    fs::write(
        &path,
        "val globalKey = \"status\"\n\
         val read = fn(prefix) {\n\
           val suffix = \"tus\"\n\
           fn(value) match {\n\
             {[prefix + suffix]: result, ...rest} => result + rest.extra\n\
             _ => \"missing\"\n\
           }\n\
         }\n\
         val destructure = fn(key, value) { val {[key]: found} = value; found }\n\
         val exact = match ({[1]: \"one\"}) { {|[1]: result|} => result; _ => \"missing\" }\n\
         val alternative = match ({status: \"ready\"}) {\n\
           {[globalKey]: \"ok\"}, {[globalKey]: \"ready\"} => \"alternative\"\n\
           _ => \"missing\"\n\
         }\n\
         var evaluations = 0\n\
         val key = fn() { evaluations = evaluations + 1; \"status\" }\n\
         val evaluated = match ({status: \"ready\"}) { {[key()]: \"ready\"} => \"once\" }\n\
         println(destructure(globalKey, {status: \"Slug\"}), read(\"sta\")({status: \"ok\", extra: \"!\"}), exact, alternative, evaluated, evaluations)\n",
    )
    .expect("write computed map pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run computed map pattern source");
    fs::remove_file(path).expect("remove computed map pattern source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug ok! one alternative once 1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_unhashable_computed_map_pattern_keys_from_source() {
    let path = fixture_path("invalid-computed-map-pattern-key");
    fs::write(&path, "match ({status: \"ok\"}) { {[[]]: _} => true }\n")
        .expect("write invalid computed map pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid computed map pattern source");
    fs::remove_file(path).expect("remove invalid computed map pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: runtime error: list cannot be used as a map key")
    );
}

#[test]
fn requires_a_value_pattern_after_a_computed_map_key() {
    let path = fixture_path("computed-map-pattern-shorthand");
    fs::write(
        &path,
        "match ({status: \"ok\"}) { {[\"status\"]} => true }\n",
    )
    .expect("write computed map pattern shorthand");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run computed map pattern shorthand");
    fs::remove_file(path).expect("remove computed map pattern shorthand");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected : after computed map pattern key")
    );
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
fn captures_remaining_map_entries_in_match_and_destructuring() {
    let path = fixture_path("map-rest-patterns");
    fs::write(
        &path,
        "val {name, ...rest} = {name: \"Slug\", status: \"ok\", active: true}\n\
         val describe = fn(user) match {\n\
           {name, ...remaining} => name + \":\" + remaining.status\n\
           _ => \"missing\"\n\
         }\n\
         println(name, rest.status, rest.active, describe({name: \"Eve\", status: \"ready\", age: 3}))\n",
    )
    .expect("write map rest source");
    let output = slug().arg(&path).output().expect("run map rest source");
    fs::remove_file(path).expect("remove map rest source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug ok true Eve:ready\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_map_all_selection_in_match_cases_without_panicking() {
    let path = fixture_path("map-all-match-case");
    fs::write(&path, "val map = {value: 1}\nmatch map { {*} => value }\n")
        .expect("write invalid map-all match source");

    for type_check in [false, true] {
        let mut command = slug();
        if type_check {
            command.arg("-type-check");
        }
        let output = command
            .arg(&path)
            .output()
            .expect("run invalid map-all match source");

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with("slug: semantic error: {*} is only valid in a top-level declaration")
        );
    }

    fs::remove_file(path).expect("remove invalid map-all match source");
}

#[test]
fn discards_anonymous_list_and_map_pattern_remainders() {
    let path = fixture_path("anonymous-rest-patterns");
    fs::write(
        &path,
        "val [first, ...] = [1, 2, 3]\n\
         val list_head = fn(values) match {\n\
           [head, ...] => head\n\
           _ => nil\n\
         }\n\
         val map_name = fn(value) match {\n\
           {name, ...} => name\n\
           _ => nil\n\
         }\n\
         println(first, list_head([4, 5]), map_name({name: \"Slug\", extra: true}))\n",
    )
    .expect("write anonymous rest pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run anonymous rest pattern source");
    fs::remove_file(path).expect("remove anonymous rest pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1 4 Slug\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_non_final_anonymous_list_rest_patterns() {
    let path = fixture_path("non-final-anonymous-list-rest");
    fs::write(&path, "val [head, ..., tail] = [1, 2, 3]\n")
        .expect("write non-final anonymous list rest source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-final anonymous list rest source");
    fs::remove_file(path).expect("remove non-final anonymous list rest source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: list spread pattern must be final")
    );
}

#[test]
fn rejects_anonymous_rest_in_exact_map_patterns() {
    let path = fixture_path("anonymous-exact-map-rest");
    fs::write(&path, "val {|name, ...|} = {name: \"Slug\"}\n")
        .expect("write exact map anonymous rest source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run exact map anonymous rest source");
    fs::remove_file(path).expect("remove exact map anonymous rest source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: exact map patterns cannot contain a spread pattern")
    );
}

#[test]
fn exact_map_patterns_reject_extra_entries() {
    let path = fixture_path("exact-map-patterns");
    fs::write(
        &path,
        "val describe = fn(user) match {\n\
           {|name: \"Slug\", active: true|} => \"exact\"\n\
           {name} => name\n\
           _ => \"other\"\n\
         }\n\
         val {|name|} = {name: \"Slug\"}\n\
         println(describe({name: \"Slug\", active: true}), describe({name: \"Slug\", active: true, extra: 1}), name)\n",
    )
    .expect("write exact map pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run exact map pattern source");
    fs::remove_file(path).expect("remove exact map pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "exact Slug Slug\n"
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
    fs::remove_file(&path).expect("remove invalid destructuring source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: destructuring pattern did not match\n"));
    assert!(stderr.contains(&format!("    --> {}:1:14\n", path.display())));
    assert!(stderr.ends_with("\n  in main\n"));
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

    let missing_parameter = fixture_path("missing-parameter-recur");
    fs::write(
        &missing_parameter,
        "val invalid = fn(n) { recur() }\ninvalid(1)\n",
    )
    .expect("write missing-parameter recur source");
    let output = slug()
        .arg(&missing_parameter)
        .output()
        .expect("run missing-parameter recur source");
    fs::remove_file(missing_parameter).expect("remove missing-parameter recur source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: runtime error: missing required parameter `n`")
    );
}

#[test]
fn recur_uses_the_ordinary_call_binding_rules() {
    let path = fixture_path("recur-call-binding");
    fs::write(
        &path,
        "val defaulted = fn(value = 7) { if (value == 7) { value } else { recur() } }\n\
         val variadic = fn(first = 1, ...rest) {\n\
           if (first == 0) { rest } else { recur(first = 0, rest = rest) }\n\
         }\n\
         val matched = fn(value = 2, ...rest) match {\n\
           [0, rest] => rest\n\
           [value, rest] => recur(value - 1, ...rest)\n\
         }\n\
         println(defaulted(4), variadic(3, 4, 5), matched(2, 8, 9))\n",
    )
    .expect("write recur binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recur binding source");
    fs::remove_file(path).expect("remove recur binding source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "7 [4, 5] [8, 9]\n"
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
    fs::remove_file(&path).expect("remove invalid return source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: semantic error: return is only valid inside a function\n"));
    assert!(stderr.contains(&format!("    --> {}:2:1\n", path.display())));
}

#[test]
fn reports_uncaught_throws_with_their_source_location_and_call_frames() {
    let path = fixture_path("throw");
    fs::write(
        &path,
        "val fail = fn() {\n\
           throw [\"bad\", 7]\n\
         }\n\
         fail()\n",
    )
    .expect("write throwing source");
    let output = slug().arg(&path).output().expect("run throwing source");
    fs::remove_file(&path).expect("remove throwing source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: uncaught throw: [\"bad\", 7]\n"));
    assert!(stderr.contains(&format!("    --> {}:2:1\n", path.display())));
    assert!(stderr.contains(&format!("\n  in <fn #0> at {}:4:", path.display())));
    assert!(stderr.ends_with("\n  in main\n"));
}

#[test]
fn renders_active_error_stacktraces_with_recursive_causes() {
    let path = fixture_path("stacktrace-causes");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer onerror(err) { println(stacktrace(err)) }\n\
           defer onerror(err) { throw \"replacement\" }\n\
           throw \"original\"\n\
         }\n\
         fail()\n",
    )
    .expect("write stacktrace source");
    let output = slug().arg(&path).output().expect("run stacktrace source");
    fs::remove_file(&path).expect("remove stacktrace source");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.starts_with("runtime error: uncaught throw: replacement\n"));
    assert!(stdout.contains(&format!("  at {}:3:22\n", path.display())));
    assert!(stdout.contains(&format!("  in <fn #2> at {}:6:1\n", path.display())));
    assert!(stdout.contains("caused by:\n  runtime error: uncaught throw: original\n"));
    assert!(stdout.contains(&format!("    at {}:4:1\n", path.display())));
    assert!(stdout.ends_with("    in main\n"));
}

#[test]
fn rejects_stacktrace_calls_outside_active_error_handling() {
    let path = fixture_path("stacktrace-inactive");
    fs::write(&path, "stacktrace(\"no error\")\n").expect("write inactive stacktrace source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run inactive stacktrace source");
    fs::remove_file(&path).expect("remove inactive stacktrace source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with(
                "slug: runtime error: `stacktrace` is only valid while handling an active error\n"
            )
    );
}

#[test]
fn runs_deferred_actions_in_lifo_order_on_normal_return() {
    let path = fixture_path("defer");
    fs::write(
        &path,
        "val finish = fn(shouldThrow) {\n\
           defer println(\"outer\")\n\
           {\n\
             defer println(\"inner\")\n\
             if (shouldThrow) { throw \"stop\" }\n\
             42\n\
           }\n\
         }\n\
         println(finish(false))\n",
    )
    .expect("write deferred source");
    let output = slug().arg(&path).output().expect("run deferred source");
    fs::remove_file(path).expect("remove deferred source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "inner\nouter\n42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn runs_deferred_actions_before_an_uncaught_throw() {
    let path = fixture_path("defer-throw");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer println(\"first\")\n\
           defer println(\"second\")\n\
           throw \"stop\"\n\
         }\n\
         fail()\n",
    )
    .expect("write throwing deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run throwing deferred source");
    fs::remove_file(path).expect("remove throwing deferred source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "second\nfirst\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("uncaught throw: stop")
    );
}

#[test]
fn runs_deferred_actions_before_a_runtime_fault() {
    let path = fixture_path("defer-runtime-fault");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer println(\"cleanup\")\n\
           1 / 0\n\
         }\n\
         fail()\n",
    )
    .expect("write faulting deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run faulting deferred source");
    fs::remove_file(path).expect("remove faulting deferred source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "cleanup\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("division by zero")
    );
}

#[test]
fn runs_onsuccess_actions_only_after_normal_completion() {
    let path = fixture_path("defer-onsuccess");
    fs::write(
        &path,
        "val complete = fn() { defer println(\"always\")\n defer onsuccess println(\"success\")\n 1 }\nprintln(complete())\n",
    ).expect("write onsuccess source");
    let output = slug().arg(&path).output().expect("run onsuccess source");
    fs::remove_file(path).expect("remove onsuccess source");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "success\nalways\n1\n"
    );
}

#[test]
fn recovers_errors_with_defer_onerror_and_resumes_the_caller() {
    let path = fixture_path("defer-onerror");
    fs::write(
        &path,
        "val fail = fn(pass) {\n\
           defer println(\"always\")\n\
           defer onerror(err) { println(\"caught\", err)\n 10 }\n\
           defer onsuccess println(\"success\")\n\
           if (pass) { \"ok\" } else { throw \"bad\" }\n\
         }\n\
         println(fail(false))\n",
    )
    .expect("write recovering deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recovering deferred source");
    fs::remove_file(path).expect("remove recovering deferred source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "caught bad\nalways\n10\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn exposes_checked_faults_to_defer_onerror_as_structured_values() {
    let path = fixture_path("defer-onerror-fault");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer onerror(err) { println(err.type, err.msg, err.data)\n 42 }\n\
           1 / 0\n\
         }\n\
         println(\"after\", fail())\n",
    )
    .expect("write fault-recovering deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run fault-recovering deferred source");
    fs::remove_file(path).expect("remove fault-recovering deferred source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "divide_by_zero division by zero nil\nafter 42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn recovery_preserves_the_callers_active_scopes() {
    let path = fixture_path("defer-onerror-caller-scope");
    fs::write(
        &path,
        "val callee = fn() { defer onerror(err) { 1 }\n throw \"bad\" }\n\
         val caller = fn() {\n\
           { callee() }\n\
           7\n\
         }\n\
         println(caller())\n",
    )
    .expect("write caller-scope recovery source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run caller-scope recovery source");
    fs::remove_file(path).expect("remove caller-scope recovery source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "7\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rethrowing_deferred_handlers_run_older_pending_cleanup() {
    let path = fixture_path("defer-onerror-rethrow-cleanup");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer println(\"first\")\n\
           defer println(\"second\")\n\
           defer onerror(err) { println(\"handler\")\n throw \"replacement\" }\n\
           throw \"original\"\n\
         }\n\
         fail()\n",
    )
    .expect("write rethrowing deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run rethrowing deferred source");
    fs::remove_file(path).expect("remove rethrowing deferred source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "handler\nsecond\nfirst\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("uncaught throw: replacement")
    );
}

#[test]
fn recur_exits_nested_scopes_before_starting_the_next_iteration() {
    let path = fixture_path("recur-nested-defer");
    fs::write(
        &path,
        "val count = fn(n) {\n\
           {\n\
             defer println(n)\n\
             if (n == 0) { 0 } else { recur(n - 1) }\n\
           }\n\
         }\n\
         println(count(2))\n",
    )
    .expect("write recur cleanup source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recur cleanup source");
    fs::remove_file(path).expect("remove recur cleanup source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "2\n1\n0\n0\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn deferred_actions_run_their_own_pending_cleanup_before_returning() {
    let path = fixture_path("deferred-action-cleanup");
    fs::write(
        &path,
        "val complete = fn() {\n\
           defer {\n\
             defer println(\"inner\")\n\
             println(\"outer\")\n\
             return nil\n\
           }\n\
           1\n\
         }\n\
         println(complete())\n",
    )
    .expect("write nested deferred cleanup source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nested deferred cleanup source");
    fs::remove_file(path).expect("remove nested deferred cleanup source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "outer\ninner\n1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn non_tail_match_discards_its_subject_before_producing_a_result() {
    let path = fixture_path("non-tail-match");
    fs::write(&path, "println(match 1 { 1 => \"yes\" })\n").expect("write non-tail match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-tail match source");
    fs::remove_file(path).expect("remove non-tail match source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "yes\n"
    );
    assert!(output.stderr.is_empty());
}
