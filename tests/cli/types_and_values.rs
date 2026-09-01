use super::*;

#[test]
fn pipes_values_into_calls_and_subjectless_matches() {
    let path = fixture_path("pipeline");
    fs::write(
        &path,
        "val add = fn(first, second) { first + second }\nval double = fn(value) { value * 2 }\nval total = 2 /> add(3) /> double\nval first = [1, 2, 3] /> match {\n  [head, ...] => head\n}\nprintln(total, first)\n",
    )
    .expect("write pipeline source");
    let output = slug().arg(&path).output().expect("run pipeline source");
    fs::remove_file(&path).expect("remove pipeline source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "10 1\n");

    fs::write(&path, "1 /> match 2 { _ => 3 }\n").expect("write invalid pipeline match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid pipeline match source");
    fs::remove_file(path).expect("remove invalid pipeline match source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: pipeline match must omit its subject")
    );
}

#[test]
fn matches_and_destructures_structs_by_schema_identity() {
    let path = fixture_path("struct-patterns");
    fs::write(
        &path,
        "val User = struct { name, active = true }\nval OtherUser = struct { name, active = true }\nval user = User { name: \"Slug\" }\nval describe = fn(value) match {\n  {name, active: true}: struct<User> => name\n  _ => \"other\"\n}\nval missing = fn(value) match {\n  {missing}: struct<User> => \"matched\"\n  _ => \"other\"\n}\nprintln(describe(user), describe(OtherUser { name: \"Slug\" }), missing(user), user.name)\n",
    )
    .expect("write struct pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run struct pattern source");
    fs::remove_file(&path).expect("remove struct pattern source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug other other Slug\n"
    );

    fs::write(
        &path,
        "val User = struct { name }\nmatch User { name: \"Slug\" } { {name, name}: struct<User> => name }\n",
    )
    .expect("write duplicate struct pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run duplicate struct pattern source");
    fs::remove_file(&path).expect("remove duplicate struct pattern source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: parse error: duplicate map pattern key `name`")
    );

    fs::write(
        &path,
        "val NotSchema = 1\nmatch (nil) { _: struct<NotSchema> => true }\n",
    )
    .expect("write invalid struct pattern schema source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid struct pattern schema source");
    fs::remove_file(path).expect("remove invalid struct pattern schema source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: struct match type must be a struct schema")
    );
}

#[test]
fn matches_reifiable_type_constraints_and_narrows_case_bindings() {
    let path = fixture_path("match-type-constraints");
    fs::write(
        &path,
        "val Marker = struct {}\n\
         val classify = fn(value) match {\n\
           b: bool => if (b) { \"true\" } else { \"false\" }\n\
           {|first, second|}: map<str, str> => first + second\n\
           [head, ...]: list<num> => head\n\
           _: str|bytes => \"text\"\n\
           _: struct => \"struct\"\n\
           _: any => \"other\"\n\
           _ => \"nil\"\n\
         }\n\
         val takesBool = fn(value:bool) { value }\n\
         val narrowed = fn(value:any|nil) match {\n\
           b: bool => takesBool(b)\n\
           _ => false\n\
         }\n\
         val piped = true /> match { b: bool => b; _ => false }\n\
         println(classify(true), classify({first: \"a\", second: \"b\"}), classify({first: \"a\", second: 2}), classify([7, 8]), classify([true]), classify(\"Slug\"), classify(Marker {}), classify(1), classify(nil), narrowed(true), piped)\n",
    )
    .expect("write match type constraint source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run match type constraint source");
    fs::remove_file(path).expect("remove match type constraint source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true ab other 7 other text struct other nil true true\n"
    );
}

#[test]
fn distinguishes_schema_values_from_struct_instances_and_infers_nominal_types() {
    let path = fixture_path("schema-types");
    fs::write(
        &path,
        "val S: schema = struct { name: str }\n\
         val Alias = S\n\
         val s: struct<S> = S {name: \"evan\"}\n\
         val alias: struct<Alias> = Alias {name: \"ada\"}\n\
         val takesSchema = fn(value: schema) { \"narrowed\" }\n\
         val classify = fn(value) match {\n\
           _: schema => \"schema\"\n\
           _: struct => \"struct\"\n\
           _ => \"other\"\n\
         }\n\
         val narrowed = fn(value) match { found: schema => takesSchema(found); _ => \"other\" }\n\
         println(classify(S), classify(s), classify(alias), narrowed(S))\n",
    )
    .expect("write schema type source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run schema type source");
    fs::remove_file(&path).expect("remove schema type source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "schema struct struct narrowed\n"
    );

    fs::write(
        &path,
        "val S = struct {}\nval Other = struct {}\nval value: struct<S> = Other {}\n",
    )
    .expect("write mismatched nominal struct source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run mismatched nominal struct source");
    fs::remove_file(&path).expect("remove mismatched nominal struct source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected struct<S>, got struct<Other>")
    );

    fs::write(&path, "val S: schema<num> = struct {}\n")
        .expect("write parameterized schema source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run parameterized schema source");
    fs::remove_file(path).expect("remove parameterized schema source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: type `schema` expects 0 argument(s)")
    );
}

#[test]
fn type_check_validates_known_operations_and_preserves_collection_results() {
    let path = fixture_path("checked-expression-operations");
    fs::write(
        &path,
        "val numbers:list<num> = [1, 2]\n\
         val labels:map<str, str> = {first: \"Slug\"}\n\
         val item:num = numbers[0]\n\
         val maybe:str|nil = labels.first\n\
         val slice:list<num> = numbers[0:1]\n\
         val joined:list<num> = numbers + [3]\n\
         val appended:list<num> = joined :+ 4\n\
         val prepended:list<num> = 0 +: appended\n\
         val repeated:str = \"go\" * 2\n\
         val rendered:str = \"list of two + \" + len(numbers)\n\
         println(item, maybe, slice, prepended, repeated, rendered)\n",
    )
    .expect("write checked expression source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run checked expression source");
    fs::remove_file(&path).expect("remove checked expression source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "1 Slug [1] [0, 1, 2, 3, 4] gogo list of two + 2\n"
    );

    for (source, expected) in [
        ("val value = \"name\" - 1\n", "expected num, got str"),
        ("val value = [1][\"first\"]\n", "expected num, got str"),
        ("val value = {name: 1}[true]\n", "expected str, got bool"),
        (
            "val value = 1[0]\n",
            "operator `[]` does not accept num and num",
        ),
        ("val value = \"name\"[0:1]\n", "expected list, got str"),
    ] {
        fs::write(&path, source).expect("write invalid checked expression source");
        let output = slug()
            .arg("-type-check")
            .arg(&path)
            .output()
            .expect("run invalid checked expression source");
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with(&format!("slug: semantic error: {expected}"))
        );
    }
    fs::remove_file(path).expect("remove invalid checked expression source");
}

#[test]
fn type_check_allows_strings_to_concatenate_lists_and_maps() {
    let path = fixture_path("string-collection-concatenation");
    fs::write(
        &path,
        "val listText:str = \"items: \" + [1, 2]\nval mapText:str = \"data: \" + {status: \"ok\"}\nprintln(listText, mapText)\n",
    )
    .expect("write string collection concatenation source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run string collection concatenation source");
    fs::remove_file(path).expect("remove string collection concatenation source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "items: [1, 2] data: {\"status\": \"ok\"}\n"
    );
}

#[test]
fn type_check_narrows_nilable_bindings_through_conditions() {
    let path = fixture_path("nil-control-flow-narrowing");
    fs::write(
        &path,
        "val use = fn(value:str):str { value + \"!\" }\n\
         val describe = fn(value:str|nil) {\n\
           if (value != nil) { use(value) } else { \"missing\" }\n\
         }\n\
         val describeEqual = fn(value:str|nil) {\n\
           if (value == nil) { \"missing\" } else { use(value) }\n\
         }\n\
         val andResult = fn(value:str|nil) { (value != nil) && use(value) }\n\
         val orResult = fn(value:str|nil) { (value == nil) || use(value) }\n\
         val guarded = fn(value:str|nil) match { candidate if candidate != nil => use(candidate); _ => \"missing\" }\n\
         val nested = fn(value:str|nil) { if (value != nil) { if (value != nil) { use(value) } else { \"impossible\" } } else { \"missing\" } }\n\
         val shadowed = fn(value:str|nil) { if (value != nil) { val value:str = \"shadow\"; use(value) } else { \"missing\" } }\n\
         val assigned = fn(flag:bool) {\n\
           var value:str|nil = nil\n\
           if (flag) { value = \"left\" } else { value = \"right\" }\n\
           use(value)\n\
         }\n\
         println(describe(\"Slug\"), describe(nil), describeEqual(\"Slug\"), andResult(\"go\"), orResult(\"go\"), guarded(\"match\"), nested(\"nested\"), shadowed(\"outer\"), assigned(true))\n",
    )
    .expect("write nil narrowing source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run nil narrowing source");
    fs::remove_file(&path).expect("remove nil narrowing source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug! missing Slug! go! go! match! nested! shadow! left!\n"
    );

    fs::write(
        &path,
        "val use = fn(value:str):str { value }\n\
         val invalid = fn(value:str|nil) {\n\
           if (value != nil) { use(value) }\n\
           use(value)\n\
         }\n",
    )
    .expect("write escaped nil narrowing source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run escaped nil narrowing source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got str|nil")
    );

    fs::write(
        &path,
        "val use = fn(value:str):str { value }\n\
         val incomplete = fn(flag:bool) {\n\
           var value:str|nil = nil\n\
           if (flag) { value = \"only\" }\n\
           use(value)\n\
         }\n",
    )
    .expect("write incomplete type-state source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run incomplete type-state source");
    fs::remove_file(path).expect("remove nil narrowing sources");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got str|nil")
    );
}

#[test]
fn type_check_reports_closed_match_coverage_without_changing_dynamic_matches() {
    let path = fixture_path("match-coverage");
    fs::write(
        &path,
        "val S = struct {}\n\
         val T = struct {}\n\
         val classify = fn(value:str|nil) { match value { _:str => \"str\"; _:nil => \"nil\" } }\n\
         val guarded = fn(value:str|nil) { match value { _:str if true => \"guarded\"; _:str => \"str\"; _:nil => \"nil\" } }\n\
         val schema = fn(value:struct<S>|struct<T>) { match value { _:struct<S> => \"s\"; _:struct<T> => \"t\" } }\n\
         val structural = fn(values:list<num>) { match values { [head, ...] => head } }\n\
         println(classify(\"Slug\"), classify(nil), guarded(\"Slug\"), schema(S {}), schema(T {}), structural([]))\n",
    )
    .expect("write covered match source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run covered match source");
    fs::remove_file(&path).expect("remove covered match source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "str nil guarded s t nil\n"
    );

    for (source, expected) in [
        (
            "val value:str|nil = nil\nmatch value { _:str => \"str\" }\n",
            "non-exhaustive match; missing nil",
        ),
        (
            "val value:str|nil = nil\nmatch value { _:str => \"str\"; _:num => \"num\"; _:nil => \"nil\" }\n",
            "match case cannot match remaining type nil",
        ),
        (
            "val value:str|nil = nil\nmatch value { _:str => \"str\"; _:nil => \"nil\"; _ => \"never\" }\n",
            "match case is unreachable",
        ),
    ] {
        fs::write(&path, source).expect("write invalid match coverage source");
        let output = slug()
            .arg("-type-check")
            .arg(&path)
            .output()
            .expect("run invalid match coverage source");
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with(&format!("slug: semantic error: {expected}"))
        );
    }
    fs::remove_file(path).expect("remove invalid match coverage source");
}

#[test]
fn type_check_uses_known_schema_fields_for_struct_values() {
    let path = fixture_path("schema-field-types");
    fs::write(
        &path,
        "val User = struct { name:str, age:num = 0 }\n\
         val Alias = User\n\
         val user:struct<User> = User {name: \"Slug\"}\n\
         val alias:struct<Alias> = Alias {name: \"Ada\", age: 42}\n\
         val same_schema:struct<Alias> = user\n\
         val updated:struct<User> = user copy {age: 1}\n\
         val name:str = user.name\n\
         val age:num = updated.age\n\
         { val User = struct { name:num }\n\
           val shadow_safe:str = user.name\n\
           println(shadow_safe) }\n\
         println(name, alias.name, same_schema.name, age)\n",
    )
    .expect("write typed schema field source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run typed schema field source");
    fs::remove_file(&path).expect("remove typed schema field source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug\nSlug Ada Slug 1\n"
    );

    for (source, expected) in [
        (
            "val User = struct { name:str }\nUser {name: 1}\n",
            "expected str, got num",
        ),
        (
            "val User = struct { name:str }\nUser {name: \"Slug\", extra: true}\n",
            "struct schema has no field `extra`",
        ),
        (
            "val User = struct { name:str }\nUser {}\n",
            "missing required struct field `name`",
        ),
        (
            "val User = struct { name:str, age:num = 0 }\nval user = User {name: \"Slug\"}\nuser copy {age: \"old\"}\n",
            "expected num, got str",
        ),
        (
            "val User = struct { name:str }\nval user = User {name: \"Slug\"}\nuser.age\n",
            "struct has no field `age`",
        ),
        (
            "val NotSchema = 1\nval value:struct<NotSchema> = nil\n",
            "struct type argument `NotSchema` must resolve to a schema binding",
        ),
        (
            "val User = struct { name:str }\nval user = User {name: \"Slug\"}\nval takes_num = fn(value:num) { value }\nmatch user { {name}: struct<User> => takes_num(name) }\n",
            "expected num, got str",
        ),
        (
            "val User = struct { name:str }\nval user = User {name: \"Slug\"}\nval takes_num = fn(value:num) { value }\nval {name} = user\ntakes_num(name)\n",
            "expected num, got str",
        ),
        (
            "val User = struct { name:str }\nval user = User {name: \"Slug\"}\nval takes_num = fn(value:num) { value }\nval check = fn(value:struct<User>) { val {name} = value; takes_num(name) }\ncheck(user)\n",
            "expected num, got str",
        ),
    ] {
        fs::write(&path, source).expect("write invalid typed schema field source");
        let output = slug()
            .arg("-type-check")
            .arg(&path)
            .output()
            .expect("run invalid typed schema field source");
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with(&format!("slug: semantic error: {expected}"))
        );
    }
    fs::remove_file(path).expect("remove invalid typed schema field source");
}

#[test]
fn rejects_non_reifiable_match_type_constraints() {
    let path = fixture_path("invalid-match-type-constraint");
    fs::write(&path, "match value { _: fn<num, num> => true }\n")
        .expect("write invalid match type constraint source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid match type constraint source");
    fs::remove_file(path).expect("remove invalid match type constraint source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: match type constraint is not runtime-checkable")
    );
}

#[test]
fn evaluates_checked_bitwise_and_shift_operators() {
    let path = fixture_path("bitwise-and-shifts");
    fs::write(&path, "println(6 & 3, 4 | 1, 6 ^ 3, ~0, 1 << 4, -8 >> 2)\n")
        .expect("write bitwise source");
    let output = slug().arg(&path).output().expect("run bitwise source");
    fs::remove_file(&path).expect("remove bitwise source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "2 5 5 -1 16 -2\n"
    );

    for source in ["1 << -1\n", "1 << 64\n", "1.5 & 1\n", "~true\n"] {
        fs::write(&path, source).expect("write invalid bitwise source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid bitwise source");
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with("slug: runtime error:")
        );
    }
    fs::remove_file(path).expect("remove invalid bitwise source");
}

#[test]
fn appends_and_prepends_list_values_with_checked_operands() {
    let path = fixture_path("list-concatenation");
    fs::write(
        &path,
        "val original = [1, 2]\nval appended = original :+ 3\nval combined = original + [3, 4]\nprintln(original, appended, 0 +: original, combined)\n",
    )
        .expect("write list concatenation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run list concatenation source");
    fs::remove_file(&path).expect("remove list concatenation source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[1, 2] [1, 2, 3] [0, 1, 2] [1, 2, 3, 4]\n"
    );

    fs::write(&path, "1 :+ 2\n").expect("write invalid list concatenation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid list concatenation source");
    fs::remove_file(path).expect("remove invalid list concatenation source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: left operand of :+ must be a list")
    );
}

#[test]
fn parses_decimal_hexadecimal_and_byte_literals() {
    let path = fixture_path("numeric-and-byte-literals");
    fs::write(
        &path,
        "println(1_000, 1.5, 2e3, 1.25e-2, 0x10, 0x_ff, 0x\"414243\", len(0x\"\"))\n",
    )
    .expect("write numeric and byte literals");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run numeric and byte literals");
    fs::remove_file(path).expect("remove numeric and byte literals");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1000 1.5 2000 0.0125 16 255 0x\"414243\" 0\n"
    );
}

#[test]
fn rejects_malformed_hexadecimal_and_byte_literals_with_locations() {
    for (kind, source, message) in [
        ("empty-hex", "0x\n", "expected hexadecimal digit"),
        (
            "odd-byte-literal",
            "0x\"f\"\n",
            "byte literal must contain complete hexadecimal byte pairs",
        ),
        (
            "invalid-byte-literal",
            "0x\"gg\"\n",
            "invalid hexadecimal digit in byte literal",
        ),
        (
            "double-decimal-separator",
            "1__000\n",
            "invalid number separator",
        ),
        (
            "trailing-hexadecimal-separator",
            "0xff_\n",
            "invalid hexadecimal number separator",
        ),
        ("missing-exponent", "1e\n", "expected exponent digit"),
    ] {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write malformed literal source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run malformed literal source");
        fs::remove_file(&path).expect("remove malformed literal source");

        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
        assert!(stderr.starts_with("slug: parse error:"), "{stderr}");
        assert!(stderr.contains(message), "{stderr}");
        assert!(
            stderr.contains(&format!("    --> {}:1:1\n", path.display())),
            "{stderr}"
        );
    }
}
