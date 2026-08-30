use super::*;

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
fn parses_raw_triple_quoted_and_extended_escaped_strings() {
    let path = fixture_path("string-forms");
    fs::write(
        &path,
        "val name = \"Slug\"\nprintln('C:\\Program Files\\Slug', \"escaped \\$ and \\{\", \"\"\"\nfirst\n  second\n\"\"\", '''\nliteral $name\n''')\n",
    )
    .expect("write string forms");
    let output = slug().arg(&path).output().expect("run string forms");
    fs::remove_file(path).expect("remove string forms");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "C:\\Program Files\\Slug escaped $ and \\{ first\n  second literal $name\n"
    );
}

#[test]
fn elides_newlines_adjacent_to_triple_string_delimiters() {
    let path = fixture_path("triple-string-final-newline");
    fs::write(&path, "println(\"\"\"\nfirst\nsecond\n\"\"\")\n")
        .expect("write triple string source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run triple string source");
    fs::remove_file(path).expect("remove triple string source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "first\nsecond\n");
}

#[test]
fn parses_one_to_three_digit_octal_string_escapes() {
    let path = fixture_path("octal-string-escapes");
    fs::write(&path, "println(\"\\101\\40\\141\")\n").expect("write octal escapes");
    let output = slug().arg(&path).output().expect("run octal escapes");
    fs::remove_file(path).expect("remove octal escapes");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "A a\n");
}

#[test]
fn interpolates_identifier_values_in_non_raw_strings() {
    let path = fixture_path("identifier-interpolation");
    fs::write(
        &path,
        "val name = \"Slug\"\nval total = 42\nprintln(\"Hello $name\", \"Total: $total\", '$name')\n",
    )
    .expect("write interpolation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run interpolation source");
    fs::remove_file(&path).expect("remove interpolation source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Hello Slug Total: 42 $name\n"
    );

    fs::write(&path, "\"$missing\"\n").expect("write unknown interpolation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run unknown interpolation source");
    fs::remove_file(path).expect("remove unknown interpolation source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error:")
    );
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
fn slices_lists_with_an_omitted_start() {
    let path = fixture_path("list-slices");
    fs::write(
        &path,
        "val values = [10, 20, 30, 40, 50]\n\
         println(values[:2], values[0:2], values[1:4:2], values[-3:])\n",
    )
    .expect("write slice source");
    let output = slug().arg(&path).output().expect("run slice source");
    fs::remove_file(path).expect("remove slice source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "[10, 20] [10, 20] [20, 40] [30, 40, 50]\n"
    );
}

#[test]
fn expands_list_and_call_spreads_in_source_order() {
    let path = fixture_path("spreads");
    fs::write(
        &path,
        "var order = \"\"\n\
         val mark = fn(value) { order = order + value; value }\n\
         val values = [mark(\"a\"), ...[mark(\"b\")], ...[mark(\"c\")], mark(\"d\")]\n\
         val collect = fn(first, second, third, fourth) { first + second + third + fourth }\n\
         println(values, collect(...[mark(\"e\")], ...[mark(\"f\")], mark(\"g\"), mark(\"h\")), order)\n",
    )
    .expect("write spread source");
    let output = slug().arg(&path).output().expect("run spread source");
    fs::remove_file(path).expect("remove spread source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "[\"a\", \"b\", \"c\", \"d\"] efgh abcdefgh\n"
    );
}

#[test]
fn binds_named_source_arguments_and_reports_binding_errors() {
    let path = fixture_path("named-arguments");
    fs::write(
        &path,
        "val format = fn(first, second, third) { first + second + third }\n\
         println(format(first = \"a\", second = \"b\", third = \"c\"), format(\"a\", third = \"c\", second = \"b\"))\n",
    )
    .expect("write named argument source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run named argument source");
    fs::remove_file(path).expect("remove named argument source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "abc abc\n");

    for (kind, source) in [
        (
            "unknown-named-argument",
            "val f = fn(value) { value }\nf(other = 1)\n",
        ),
        (
            "duplicate-named-argument",
            "val f = fn(value) { value }\nf(value = 1, value = 2)\n",
        ),
        (
            "missing-required-argument",
            "val f = fn(value) { value }\nf()\n",
        ),
        (
            "excess-positional-arguments",
            "val f = fn(value) { value }\nf(1, 2)\n",
        ),
    ] {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid named argument source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid named argument source");
        fs::remove_file(path).expect("remove invalid named argument source");
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.starts_with("slug: semantic error: no matching overload for `f`"),
            "{stderr}"
        );
    }
}

#[test]
fn combines_distinct_local_function_declarations_into_overloads() {
    let path = fixture_path("local-overloads");
    fs::write(
        &path,
        "val render = fn(value:num):str { \"number\" }\n\
         val render = fn(value:str):str { \"text\" }\n\
         println(render(1), render(\"x\"))\n\
         val main = fn() {\n\
           val render = fn(value:num):str { \"nested-number\" }\n\
           val render = fn(value:str):str { \"nested-text\" }\n\
           println(render(2), render(\"y\"))\n\
         }\n",
    )
    .expect("write local overload source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run local overload source");
    fs::remove_file(path).expect("remove local overload source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "number text\nnested-number nested-text\n"
    );
}

#[test]
fn rejects_duplicate_local_callable_signatures() {
    let path = fixture_path("duplicate-local-overload");
    fs::write(
        &path,
        "val render = fn(value:num) { value }\nval render = fn(value:num) { value }\n",
    )
    .expect("write duplicate local overload source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run duplicate local overload source");
    fs::remove_file(path).expect("remove local overload source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: duplicate callable signature for `render`")
    );
}

#[test]
fn rejects_duplicate_function_parameter_names() {
    let path = fixture_path("duplicate-parameter");
    fs::write(&path, "val duplicate = fn(value, value) { value }\n")
        .expect("write duplicate parameter source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run duplicate parameter source");
    fs::remove_file(path).expect("remove duplicate parameter source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: duplicate parameter 'value'")
    );
}

#[test]
fn binds_final_variadic_parameters() {
    let path = fixture_path("variadic-parameters");
    fs::write(
        &path,
        "val collect = fn(first, ...rest) { [first, rest] }\n\
         println(collect(1, 2, 3), collect(1), collect(first = 1, rest = [2, 3]))\n",
    )
    .expect("write variadic source");
    let output = slug().arg(&path).output().expect("run variadic source");
    fs::remove_file(path).expect("remove variadic source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[1, [2, 3]] [1, []] [1, [2, 3]]\n"
    );

    let path = fixture_path("non-list-named-variadic");
    fs::write(
        &path,
        "val collect = fn(...rest) { rest }\ncollect(rest = 1)\n",
    )
    .expect("write invalid variadic source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid variadic source");
    fs::remove_file(path).expect("remove invalid variadic source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: variadic parameter `rest` expects a list")
    );
}

#[test]
fn evaluates_omitted_parameter_defaults_in_the_callee() {
    let path = fixture_path("default-parameters");
    fs::write(
        &path,
        "val suffix = \"!\"\nval greet = fn(name = \"Slug\", ending = suffix) { name + ending }\nprintln(greet(), greet(name = \"Ada\"), greet(\"Rust\", \"?\"))\n",
    )
    .expect("write default parameter source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run default parameter source");
    fs::remove_file(path).expect("remove default parameter source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug! Ada! Rust?\n"
    );
}

#[test]
fn default_expressions_capture_the_function_defining_environment() {
    let path = fixture_path("default-closure-environment");
    fs::write(
        &path,
        "val make = fn(prefix) { fn(suffix = prefix) { suffix } }\n\
         val fromMaker = make(\"captured\")\n\
         val caller = fn(prefix) { fromMaker() }\n\
         println(caller(\"caller\"), fromMaker(suffix = \"explicit\"))\n",
    )
    .expect("write default closure source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run default closure source");
    fs::remove_file(path).expect("remove default closure source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "captured explicit\n"
    );
}

#[test]
fn function_match_bodies_observe_bound_defaults_and_variadics() {
    let path = fixture_path("function-match-call-binding");
    fs::write(
        &path,
        "val classify = fn(first = 1, ...rest) match {\n\
           [1, []] => \"default\"\n\
           [1, [2, 3]] => \"spread\"\n\
           _ => \"other\"\n\
         }\n\
         println(classify(), classify(1, 2, 3), classify(9))\n",
    )
    .expect("write function match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run function match source");
    fs::remove_file(path).expect("remove function match source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "default spread other\n"
    );
}

#[test]
fn rejects_non_list_source_spreads() {
    for (kind, source, expected) in [
        (
            "non-list-call-spread",
            "println(...1)\n",
            "slug: runtime error: call spread expects a list",
        ),
        (
            "non-list-literal-spread",
            "[...1]\n",
            "slug: runtime error: list spread expects a list",
        ),
    ] {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid spread source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid spread source");
        fs::remove_file(path).expect("remove invalid spread source");
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
fn rejects_spread_arguments_for_statically_known_overloads() {
    for (kind, source) in [
        (
            "overloaded-call-spread",
            "val render = fn(value:num) { value }\n\
             val render = fn(value:str) { value }\n\
             val values = [1]\n\
             render(...values)\n",
        ),
        (
            "overloaded-pipeline-spread",
            "val render = fn(first:num, second:num) { first + second }\n\
             val render = fn(first:num, second:str) { first }\n\
             val values = [2]\n\
             1 /> render(...values)\n",
        ),
    ] {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write overloaded spread source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run overloaded spread source");
        fs::remove_file(path).expect("remove overloaded spread source");

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().starts_with(
            "slug: semantic error: cannot resolve overload `render` with spread arguments"
        ));
    }
}

#[test]
fn constructs_and_compares_struct_values_with_stored_defaults() {
    let path = fixture_path("struct-foundation");
    fs::write(
        &path,
        "var evaluations = 0\n\
         val User = struct {\n\
           name,\n\
           sequence = { evaluations = evaluations + 1; evaluations },\n\
         }\n\
         val first = User {name: \"Slug\"}\n\
         val second = User {name: \"Slug\"}\n\
         val Other = struct {name, sequence = 1}\n\
         val other = Other {name: \"Slug\"}\n\
         println(first.name, first[\"sequence\"], evaluations, User == User, first == second, first == other, match first { _ => \"matched\" })\n",
    )
    .expect("write struct foundation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run struct foundation source");
    fs::remove_file(path).expect("remove struct foundation source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug 1 1 true true false matched\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn copies_structs_with_checked_replacement_fields() {
    let path = fixture_path("struct-copy");
    fs::write(
        &path,
        "val User = struct { name, active = true }\n\
         val first = User { name: \"Slug\" }\n\
         val second = first copy { active: false }\n\
         println(first.name, first.active, second.name, second.active, first == second)\n",
    )
    .expect("write struct copy source");
    let output = slug().arg(&path).output().expect("run struct copy source");
    fs::remove_file(&path).expect("remove struct copy source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug true Slug false false\n"
    );

    fs::write(&path, "val value = 1\nvalue copy { field: 2 }\n")
        .expect("write non-struct copy source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-struct copy source");
    fs::remove_file(path).expect("remove non-struct copy source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: cannot copy non-struct value")
    );
}

#[test]
fn reports_checked_struct_schema_construction_and_access_errors() {
    let cases = [
        (
            "missing-struct-field",
            "val User = struct {name}\nUser {}\n",
            "slug: runtime error: missing required struct field 'name'",
        ),
        (
            "unknown-struct-field",
            "val User = struct {name}\nUser {other: 1}\n",
            "slug: runtime error: struct schema has no field 'other'",
        ),
        (
            "duplicate-struct-construction-field",
            "val User = struct {name}\nUser {name: \"a\", name: \"b\"}\n",
            "slug: runtime error: duplicate struct field 'name'",
        ),
        (
            "non-schema-construction",
            "1 {name: \"a\"}\n",
            "slug: runtime error: cannot construct struct from num",
        ),
        (
            "unknown-struct-access",
            "val User = struct {name}\nval user = User {name: \"a\"}\nuser.other\n",
            "slug: runtime error: struct has no field 'other'",
        ),
        (
            "duplicate-schema-field",
            "struct {name, name}\n",
            "slug: semantic error: duplicate struct field 'name'",
        ),
    ];

    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid struct source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid struct source");
        fs::remove_file(path).expect("remove invalid struct source");
        let std::process::Output {
            status,
            stdout,
            stderr,
        } = output;
        let status = status.code();
        let stderr = String::from_utf8(stderr).expect("stderr is UTF-8");

        assert_eq!(status, Some(1));
        assert!(stdout.is_empty());
        assert!(stderr.starts_with(expected));
    }
}
