use super::*;

#[test]
fn transparent_type_aliases_preserve_underlying_types() {
    let path = fixture_path("transparent-aliases");
    fs::write(
        &path,
        "type Path = str;\n\
         type Paths = list<Path>;\n\
         type MaybePath = Path|nil;\n\
         val echo = fn(value:Path):str { value }\n\
         val classify = fn(value) match { _:Path => \"path\"; _ => \"other\" }\n\
         val S = struct { name:str }\n\
         type User = struct<S>;\n\
         val paths:Paths = [\"Slug\"]\n\
         val user:User = S {name: \"Nominal\"}\n\
         val missing:MaybePath = nil\n\
         println(echo(paths[0]), classify(paths[0]), user.name, missing == nil)\n",
    )
    .expect("write alias source");
    let output = slug().arg(&path).output().expect("run alias source");
    fs::remove_file(&path).expect("remove alias source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug path Nominal true\n"
    );
}

#[test]
fn type_aliases_reject_cycles_and_generic_use() {
    let path = fixture_path("type-alias-errors");
    fs::write(&path, "type First = Second\ntype Second = First\n").expect("write cyclic aliases");
    let output = slug().arg(&path).output().expect("run cyclic aliases");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("recursive type alias: First -> Second -> First"),
        "{stderr}"
    );

    fs::write(&path, "type Path = str\nval path:Path<str> = \"Slug\"\n")
        .expect("write generic alias use");
    let output = slug().arg(&path).output().expect("run generic alias use");
    fs::remove_file(&path).expect("remove alias source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unknown type constructor `Path`")
    );
}

#[test]
fn fieldless_enum_values_are_qualified_nominal_values() {
    let path = fixture_path("fieldless-enums");
    fs::write(
        &path,
        "enum SeekFrom {\n\
           Start,\n\
           Current,\n\
           End,\n\
         }\n\
         val from:SeekFrom = SeekFrom.Start\n\
         println(from == SeekFrom.Start, from == SeekFrom.End)\n\
         println(match from {\n\
           SeekFrom.Start => \"start\"\n\
           SeekFrom.Current => \"current\"\n\
           SeekFrom.End => \"end\"\n\
         })\n",
    )
    .expect("write enum source");
    let output = slug().arg(&path).output().expect("run enum source");
    fs::remove_file(&path).expect("remove enum source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true false\nstart\n"
    );
}

#[test]
fn type_check_requires_unguarded_coverage_of_each_enum_case() {
    let path = fixture_path("enum-coverage");
    fs::write(
        &path,
        "enum SeekFrom { Start, End }\n\
         val from:SeekFrom = SeekFrom.Start\n\
         match from {\n\
           SeekFrom.Start => 0\n\
           SeekFrom.End => 1\n\
         }\n",
    )
    .expect("write exhaustive enum source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run exhaustive enum source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    fs::write(
        &path,
        "enum SeekFrom { Start, End }\n\
         val from:SeekFrom = SeekFrom.Start\n\
         match from { SeekFrom.Start => 0 }\n",
    )
    .expect("write non-exhaustive enum source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-exhaustive enum source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("missing SeekFrom.End")
    );

    fs::write(
        &path,
        "enum SeekFrom { Start }\n\
         enum Origin { Start }\n\
         val from:SeekFrom = SeekFrom.Start\n\
         match from { Origin.Start => 0 }\n",
    )
    .expect("write mismatched enum source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run mismatched enum source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot match remaining enum cases")
    );

    fs::write(
        &path,
        "enum SeekFrom { Start, End }\n\
         val from:SeekFrom = SeekFrom.Start\n\
         match from {\n\
           SeekFrom.Start if true => 0\n\
           SeekFrom.End => 1\n\
         }\n",
    )
    .expect("write guarded enum source");
    let output = slug().arg(&path).output().expect("run guarded enum source");
    fs::remove_file(path).expect("remove enum source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("missing SeekFrom.Start")
    );
}

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
            .starts_with("slug:")
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
    let output = slug().arg(&path).output().expect("run schema type source");
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
         val letter:str = \"Slug\"[3]\n\
         val textSlice:str = \"Slug\"[1:3]\n\
         val joined:list<num> = numbers + [3]\n\
         val appended:list<num> = joined :+ 4\n\
         val prepended:list<num> = 0 +: appended\n\
         val repeated:str = \"go\" * 2\n\
         val rendered:str = \"list of two + \" + len(numbers)\n\
         println(item, maybe, slice, letter, textSlice, prepended, repeated, rendered)\n",
    )
    .expect("write checked expression source");
    let output = slug()
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
        "1 Slug [1] g lu [0, 1, 2, 3, 4] gogo list of two + 2\n"
    );

    for (source, expected) in [
        ("val value = \"name\" - 1\n", "expected num, got str"),
        ("val value = [1][\"first\"]\n", "expected num, got str"),
        ("val value = {name: 1}[true]\n", "expected str, got bool"),
        (
            "val value = 1[0]\n",
            "operator `[]` does not accept num and num",
        ),
        ("val value = 1[0:1]\n", "expected list, got num"),
    ] {
        fs::write(&path, source).expect("write invalid checked expression source");
        let output = slug()
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
fn type_check_preserves_static_map_members_across_dot_chains() {
    let path = fixture_path("checked-map-member-chains");
    fs::write(
        &path,
        "val times_two = fn(value) { value * 2 }\n\
         val m1 = {key: times_two}\n\
         val m2 = {key: m1}\n\
         val m3 = {key: m2}\n\
         println(10 /> m1.key, 10 /> m2.key.key, 10 /> m3.key.key.key)\n",
    )
    .expect("write checked map member chain source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run checked map member chain source");
    fs::remove_file(path).expect("remove checked map member chain source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "20 20 20\n");
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
         val reversedNotEqual = fn(value:str|nil) { if (nil != value) { use(value) } else { \"missing\" } }\n\
         val reversedEqual = fn(value:str|nil) { if (nil == value) { \"missing\" } else { use(value) } }\n\
         val andResult = fn(value:str|nil) { (value != nil) && use(value) }\n\
         val orResult = fn(value:str|nil) { (value == nil) || use(value) }\n\
         val composed = fn(value:str|nil) { (value != nil) && ((value != nil) && use(value)) }\n\
         val guarded = fn(value:str|nil) match { candidate if candidate != nil => use(candidate); _ => \"missing\" }\n\
         val nested = fn(value:str|nil) { if (value != nil) { if (value != nil) { use(value) } else { \"impossible\" } } else { \"missing\" } }\n\
         val shadowed = fn(value:str|nil) { if (value != nil) { val value:str = \"shadow\"; use(value) } else { \"missing\" } }\n\
         val assigned = fn(flag:bool) {\n\
           var value:str|nil = nil\n\
           if (flag) { value = \"left\" } else { value = \"right\" }\n\
           use(value)\n\
         }\n\
         println(describe(\"Slug\"), describe(nil), describeEqual(\"Slug\"), reversedNotEqual(\"reverse\"), reversedEqual(\"reverse\"), andResult(\"go\"), orResult(\"go\"), guarded(\"match\"), nested(\"nested\"), shadowed(\"outer\"), assigned(true))\n",
    )
    .expect("write nil narrowing source");
    let output = slug()
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
        "Slug! missing Slug! reverse! reverse! go! go! match! nested! shadow! left!\n"
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
fn type_check_preserves_narrowing_after_terminating_if_branches() {
    let path = fixture_path("terminating-if-narrowing");
    fs::write(
        &path,
        "val use = fn(value:str):str { value + \"!\" }\n\
         val guard = fn(value:str|nil) {\n\
           if (value == nil) { return \"missing\" }\n\
           use(value)\n\
         }\n\
         val inverse = fn(value:str|nil) {\n\
           if (value != nil) { use(value) } else { return \"missing\" }\n\
           use(value)\n\
         }\n\
         println(guard(\"Slug\"), guard(nil), inverse(\"Flow\"))\n",
    )
    .expect("write terminating-if narrowing source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run terminating-if narrowing source");
    fs::remove_file(path).expect("remove terminating-if narrowing source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug! missing Flow!\n"
    );
}

#[test]
fn type_check_treats_an_if_with_two_terminating_branches_as_terminating() {
    let path = fixture_path("terminating-if-expression");
    fs::write(
        &path,
        "val needs_str = fn(value:str):str { value }\n\
         val complete = fn(flag:bool):str {\n\
           if (flag) { return \"done\" } else { throw \"failed\" }\n\
           needs_str(42)\n\
         }\n\
         println(complete(true))\n",
    )
    .expect("write terminating-if expression source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run terminating-if expression source");
    fs::remove_file(path).expect("remove terminating-if expression source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "done\n");
}

#[test]
fn type_check_preserves_surviving_named_match_subject_facts() {
    let path = fixture_path("terminating-match-narrowing");
    fs::write(
        &path,
        "val use = fn(value:str):str { value + \"!\" }\n\
         val describe = fn(value:str|nil) {\n\
           match value { _:nil => return \"missing\"; text:str => use(text) }\n\
           use(value)\n\
         }\n\
         println(describe(\"Slug\"), describe(nil))\n",
    )
    .expect("write terminating-match narrowing source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run terminating-match narrowing source");
    fs::remove_file(path).expect("remove terminating-match narrowing source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "Slug! missing\n");
}

#[test]
fn type_check_keeps_guarded_match_facts_conservative_afterward() {
    let path = fixture_path("guarded-match-flow");
    fs::write(
        &path,
        "val use = fn(value:str):str { value }\n\
         val invalid = fn(value:str|nil) {\n\
           match value { text:str if true => use(text); _ => nil }\n\
           use(value)\n\
         }\n",
    )
    .expect("write guarded-match flow source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run guarded-match flow source");
    fs::remove_file(path).expect("remove guarded-match flow source");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got str|nil")
    );
}

#[test]
fn type_check_preserves_facts_through_nested_guard_and_match_flow() {
    let path = fixture_path("nested-flow-narrowing");
    fs::write(
        &path,
        "val use = fn(value:str):str { value + \"!\" }\n\
         val describe = fn(value:str|num|nil) {\n\
           if (value == nil) { return \"missing\" }\n\
           match value { text:str => use(text); number:num => \"number\" }\n\
         }\n\
         println(describe(\"Slug\"), describe(1), describe(nil))\n",
    )
    .expect("write nested-flow narrowing source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nested-flow narrowing source");
    fs::remove_file(path).expect("remove nested-flow narrowing source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug! number missing\n"
    );
}

#[test]
fn type_check_preserves_nominal_identity_when_narrowing_flow_facts() {
    let path = fixture_path("nominal-flow-narrowing");
    fs::write(
        &path,
        "val S = struct {}\n\
         val T = struct {}\n\
         val use = fn(value:struct<S>|struct<T>):str { \"ready\" }\n\
         val describe = fn(value:struct<S>|struct<T>|nil) {\n\
           if (value == nil) { return \"missing\" }\n\
           use(value)\n\
         }\n\
         println(describe(S {}), describe(T {}), describe(nil))\n",
    )
    .expect("write nominal-flow narrowing source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nominal-flow narrowing source");
    fs::remove_file(path).expect("remove nominal-flow narrowing source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "ready ready missing\n"
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
    fs::write(
        &path,
        "println(6 & 3, 4 | 1, 6 ^ 3, ~0, 1 << 4, -8 >> 2, 0x\"ff00\" & 0x\"0ff0\", 0x\"ff\" ^ 0x\"0000\", 255 & 0x\"0ff0\", ~0x\"00ff\")\n",
    )
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
        "2 5 5 -1 16 -2 0x\"0f00\" 0x\"ffff\" 0x\"0ff0\" 0x\"ff00\"\n"
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
                .starts_with("slug:")
        );
    }
    fs::remove_file(path).expect("remove invalid bitwise source");
}

#[test]
fn appends_and_prepends_list_values_with_checked_operands() {
    let path = fixture_path("list-concatenation");
    fs::write(
        &path,
        "val original = [1, 2]\nval appended = original :+ 3\nval combined = original + [3, 4]\nprintln(original, appended, 0 +: original, combined, len(0x\"0102\" + 0x\"0304\"), 1 +: 0x\"0203\" + 0x\"04\" :+ 5, 0x\"01\" +: 0x\"0203\" + 0x\"04\" :+ 0x\"05\", 1 +: 2 +: 3 +: 0x\"\")\n",
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
        "[1, 2] [1, 2, 3] [0, 1, 2] [1, 2, 3, 4] 4 0x\"0102030405\" 0x\"0102030405\" 0x\"010203\"\n"
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
            .starts_with("slug: semantic error:")
    );
}

#[test]
fn collection_updates_preserve_local_and_closure_aliases() {
    let path = fixture_path("collection-aliases");
    fs::write(
        &path,
        "val User = struct { name, active = true }\n\
         val list = [1, 2]\n\
         val bytes = 0x\"0203\"\n\
         val map = {stable: 1, replace: 2}\n\
         val user = User { name: \"Slug\" }\n\
         val captureList = fn() { list }\n\
         val captureBytes = fn() { bytes }\n\
         val captureMap = fn() { map }\n\
         val captureUser = fn() { user }\n\
         val appended = list :+ 3\n\
         val prepended = 0 +: list\n\
         val changedBytes = 1 +: bytes :+ 4\n\
         val merged = map + {replace: 20, added: 3}\n\
         val removed = map - \"replace\"\n\
         val copied = map copy {replace: 30, added: 4}\n\
         val changedUser = user copy {active: false}\n\
         println(list, appended, prepended, bytes, changedBytes, map.replace, merged.replace, removed.replace, copied.replace, user.active, changedUser.active, captureList(), captureBytes(), captureMap().replace, captureUser().active)\n",
    )
    .expect("write collection alias source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run collection alias source");
    fs::remove_file(path).expect("remove collection alias source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "[1, 2] [1, 2, 3] [0, 1, 2] 0x\"0203\" 0x\"01020304\" 2 20 nil 30 true false [1, 2] 0x\"0203\" 2 true\n"
    );
}

#[test]
fn list_rest_views_preserve_aliases_through_closures_and_updates() {
    let path = fixture_path("list-rest-view-aliases");
    fs::write(
        &path,
        "val original = [1, 2, 3, 4]\n\
         val [head, ...tail] = original\n\
         val capture = fn() { tail }\n\
         val changed = tail :+ 5\n\
         val sliced = original[1:3]\n\
         println(head, tail, capture(), changed, sliced, tail == [2, 3, 4], tail[1])\n",
    )
    .expect("write list rest-view alias source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run list rest-view alias source");
    fs::remove_file(path).expect("remove list rest-view alias source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1 [2, 3, 4] [2, 3, 4] [2, 3, 4, 5] [2, 3] true 3\n"
    );
}

#[test]
fn persistently_merges_removes_and_enumerates_maps() {
    let path = fixture_path("persistent-map-updates");
    fs::write(
        &path,
        "val original = {first: 1, second: 2}\n\
         val merged = original + {second: 20, third: 3}\n\
         val removed = merged - \"second\"\n\
         val repeated = merged + {second: 30}\n\
         println(original.second, merged.first, merged.second, merged.third, removed.second, repeated.second, merged == {third: 3, first: 1, second: 20}, len(keys(merged)), len(keys(removed)), len(original))\n",
    )
    .expect("write persistent map update source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run persistent map update source");
    fs::remove_file(path).expect("remove persistent map update source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "2 1 20 3 nil 30 true 3 2 2\n"
    );
}

#[test]
fn string_concatenation_never_changes_a_retained_string() {
    let path = fixture_path("string-append-aliasing");
    fs::write(
        &path,
        "val base = \"a\"\n\
         val build = fn(n, text, kept) { if (n == 0) { [text, kept] } else { recur(n - 1, text + \"b\", text) } }\n\
         val [text, kept] = build(3, base, \"\")\n\
         val number = 1\n\
         println(base, text, kept, base + number + true, base)\n",
    )
    .expect("write string aliasing source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run string aliasing source");
    fs::remove_file(path).expect("remove string aliasing source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "a abbb abb a1true a\n"
    );
}

#[test]
fn fused_local_constant_operations_match_their_standalone_forms() {
    let path = fixture_path("fused-local-constant");
    let run = |source: &str| {
        fs::write(&path, source).expect("write fused operation source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run fused operation source");
        (
            output.status.code(),
            String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            String::from_utf8(output.stderr).expect("stderr is UTF-8"),
        )
    };
    let (code, stdout, stderr) = run(
        "val numbers = fn(x) { [x + 1, x - 1, x * 2, x / 2, x % 3, x == 4, x > 1, x < 1] }\n\
         val generic = fn(x) { [x + \"!\", x == \"a\"] }\n\
         val drop = fn(m: map) { m - \"a\" }\n\
         val build = fn(n, text) { if (n == 0) { text } else { recur(n - 1, text + \"ab\") } }\n\
         println(numbers(4), numbers(4.5))\n\
         println(generic(\"a\"), drop({a: 1, b: 2}), len(build(1000, \"\")))\n",
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "[5, 3, 8, 2, 1, true, true, false] [5.5, 3.5, 9, 2.25, 1.5, false, true, false]\n\
         [\"a!\", true] {\"b\": 2} 2000\n"
    );

    // A fused operation fails exactly where its operator is.
    let (code, _, stderr) = run("val generic = fn(x) { x + \"!\" }\nprintln(generic(1))\n");
    fs::remove_file(&path).expect("remove fused operation source");
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("runtime error: expected numbers, got num and str"),
        "{stderr}"
    );
    assert!(stderr.contains(":1:25"), "{stderr}");
}

#[test]
fn full_positional_calls_skip_defaults_and_pipelines_prepend_their_value() {
    let path = fixture_path("positional-defaults");
    fs::write(
        &path,
        "val f = fn(a, b = 10, c = a + b) { [a, b, c] }\n\
         val loop = fn(n, acc = []) { if (n == 0) { acc } else { recur(n - 1, acc :+ n) } }\n\
         val v = fn(a, ...rest) { [a, rest] }\n\
         println(f(1), f(1, 2), f(1, 2, 3), loop(3), loop(2, [9]))\n\
         println(v(1), v(1, 2), 5 /> f(), 5 /> f(6, 7), 1 /> v(2, 3))\n",
    )
    .expect("write positional defaults source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run positional defaults source");
    fs::remove_file(path).expect("remove positional defaults source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "[1, 10, 11] [1, 2, 3] [1, 2, 3] [3, 2, 1] [9, 2, 1]\n\
         [1, []] [1, [2]] [5, 10, 15] [5, 6, 7] [1, [2, 3]]\n"
    );
}

#[test]
fn map_literals_reject_a_repeated_key() {
    let path = fixture_path("map-literal-repeated-keys");
    let run = |source: &str| {
        fs::write(&path, source).expect("write repeated-key source");
        let output = slug().arg(&path).output().expect("run repeated-key source");
        (
            output.status.code(),
            String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            String::from_utf8(output.stderr).expect("stderr is UTF-8"),
        )
    };

    // Constant keys are checked at compile time, under key equivalence.
    for (source, key) in [
        ("println({a: 1, \"b\": 2, a: 3})\n", "a"),
        ("println({[1]: \"int\", [1.0]: \"float\"})\n", "1"),
    ] {
        let (code, stdout, stderr) = run(source);
        assert_eq!(code, Some(1), "{source}");
        assert_eq!(stdout, "", "{source}");
        assert!(
            stderr.contains(&format!("map literal repeats key `{key}`")),
            "{stderr}"
        );
    }

    // A computed key can only be checked when the literal is built.
    let (code, stdout, stderr) =
        run("val key = \"a\"\nprintln(\"before\")\nprintln({[key]: 1, a: 2})\n");
    assert_eq!(code, Some(1));
    assert_eq!(stdout, "before\n");
    assert!(stderr.contains("map literal repeats key \"a\""), "{stderr}");

    // Distinct keys, and merge, are unaffected: merge still overwrites.
    let (code, stdout, stderr) = run("val key = \"c\"\n\
         val map = {a: 1, [key]: 2, [1]: 3, [1.5]: 4}\n\
         println(len(map), map + {a: 9} == {a: 9, c: 2, [1.0]: 3, [1.5]: 4})\n");
    fs::remove_file(&path).expect("remove repeated-key source");
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "4 true\n");
}

#[test]
fn standard_clutch_preserves_non_string_map_keys() {
    let path = fixture_path("standard-clutch-map-keys");
    fs::write(
        &path,
        "val values = {[1]: \"one\", [true]: \"yes\", name: \"Slug\", [0x\"01\"]: \"byte\"}\n\
         val updated = values + {[1.0]: \"one-point-zero\"}\n\
         val missingRemoved = updated - \"missing\"\n\
         println(updated[1], updated[1.0], updated[true], updated.name, updated[0x\"01\"], updated[\"missing\"], missingRemoved == updated, len(keys(updated)))\n",
    )
    .expect("write standard clutch source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run standard clutch source");
    fs::remove_file(path).expect("remove standard clutch source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "one-point-zero one-point-zero yes Slug byte nil true 4\n"
    );
}

#[test]
fn rejects_every_non_key_collection_value_from_source() {
    let path = fixture_path("invalid-map-key-categories");
    for (name, source, expected) in [
        ("nil", "{[nil]: 1}\n", "nil cannot be used as a map key"),
        ("list", "{[[]]: 1}\n", "list cannot be used as a map key"),
        ("map", "{[{}]: 1}\n", "map cannot be used as a map key"),
        (
            "struct",
            "val User = struct { name = \"Slug\" }\n{[User {}]: 1}\n",
            "struct cannot be used as a map key",
        ),
    ] {
        fs::write(&path, source).expect("write invalid map-key source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid map-key source");
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with(&format!("slug: runtime error: {expected}")),
            "{name}"
        );
    }
    fs::remove_file(path).expect("remove invalid map-key source");
}

#[test]
fn parses_multiline_maps_with_raw_quoted_keys() {
    let path = fixture_path("multiline-raw-quoted-map");
    fs::write(
        &path,
        "val m = {\n\
         'k1': 1,\n\
         'k2': 2\n\
         }\n\
         println(m)\n\
         println(m.k1, m.k2, len(keys(m)))\n",
    )
    .expect("write multiline map source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run multiline map source");
    fs::remove_file(path).expect("remove multiline map source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let mut lines = stdout.lines();
    let display = lines.next().expect("map display line");
    assert!(
        display.starts_with('{') && display.ends_with('}'),
        "{display}"
    );
    assert!(display.contains("\"k1\": 1"), "{display}");
    assert!(display.contains("\"k2\": 2"), "{display}");
    assert_eq!(lines.next(), Some("1 2 2"));
    assert_eq!(lines.next(), None);
    assert!(output.stderr.is_empty());
}

#[test]
fn persistently_copies_maps_with_string_key_updates() {
    let path = fixture_path("map-copy");
    fs::write(
        &path,
        "val opts = {timeout: 1000, retries: 2}\n\
         val next = opts copy {timeout: 5000, retries: 3, mode: \"fast\"}\n\
         println(opts.timeout, opts.retries, opts.mode, next.timeout, next.retries, next.mode)\n",
    )
    .expect("write map copy source");
    let output = slug().arg(&path).output().expect("run map copy source");
    fs::remove_file(path).expect("remove map copy source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1000 2 nil 5000 3 fast\n"
    );
}

#[test]
fn parses_decimal_hexadecimal_and_byte_literals() {
    let path = fixture_path("numeric-and-byte-literals");
    fs::write(
        &path,
        "val bytes = 0x\"020304\"\nprintln(1_000, 1.5, 2e3, 1.25e-2, 0x10, 0x_ff, 0x\"414243\", len(0x\"\"), bytes[0], bytes[1:])\n",
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
        "1000 1.5 2000 0.0125 16 255 0x\"414243\" 0 2 0x\"0304\"\n"
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

#[test]
fn keys_is_an_implicit_builtin_and_not_a_slug_std_export() {
    let path = fixture_path("keys-builtin");
    fs::write(
        &path,
        "val std = import(\"slug.std\")\n\
         val builtin = import(\"slug.builtin\")\n\
         val mixed = {name: \"Slug\", [7]: \"seven\", [true]: \"yes\"}\n\
         val pair = fn(values) { match values { [\"a\", \"b\"] => \"a and b\"; [\"b\", \"a\"] => \"a and b\"; _ => \"other\" } }\n\
         println(len(keys(mixed)), keys({}), pair(keys({b: 2, a: 1})))\n\
         println(builtin.keys({k: 1}), std.keys == nil)\n\
         val shadow = fn(keys) { keys }\n\
         println(shadow(\"local wins\"))\n",
    )
    .expect("write keys builtin source");
    let output = slug().arg(&path).output().expect("run keys builtin source");
    fs::remove_file(path).expect("remove keys builtin source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "3 [] a and b\n[\"k\"] true\nlocal wins\n"
    );
}

#[test]
fn keys_rejects_a_value_that_is_not_a_map() {
    let path = fixture_path("keys-not-a-map");
    fs::write(
        &path,
        "val dynamic = fn(value) { value }\nkeys(dynamic([1, 2]))\n",
    )
    .expect("write keys type-error source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run keys type-error source");
    fs::remove_file(path).expect("remove keys type-error source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("`keys` expects map, got List"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn strings_order_lexicographically_by_utf8_bytes() {
    let path = fixture_path("string-ordering");
    let run = |source: &str| {
        fs::write(&path, source).expect("write string ordering source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run string ordering source");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (code, stdout, stderr) = run(
        "println(\"apple\" < \"banana\", \"b\" > \"a\", \"a\" <= \"a\", \"a\" >= \"b\", \"Z\" < \"a\", \"ab\" < \"abc\", \"é\" > \"z\")\n\
         val before = fn(a, b) { a < b }\n\
         println(before(1, 2), before(\"pear\", \"fig\"))\n\
         val small = fn(n) { n < 10 }\n\
         val early = fn(word) { word < \"m\" }\n\
         println(small(3), early(\"kiwi\"))\n",
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "true true true false true true true\ntrue false\ntrue true\n"
    );

    // A literal on the other side still decides an unannotated parameter's type.
    let (code, _, stderr) = run("val small = fn(n) { n < 10 }\nprintln(small(\"a\"))\n");
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("semantic error: expected num, got str"),
        "{stderr}"
    );

    let (code, _, stderr) = run("println(1 < \"a\")\n");
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("semantic error: operator `<` does not accept num and str"),
        "{stderr}"
    );

    let (code, _, stderr) = run("val before = fn(a, b) { a < b }\nprintln(before(1, \"a\"))\n");
    fs::remove_file(&path).expect("remove string ordering source");
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("runtime error: expected two numbers or two strings, got num and str"),
        "{stderr}"
    );
}

#[test]
fn enum_cases_and_struct_fields_separate_with_commas_or_newlines() {
    let path = fixture_path("member-separators");
    let run = |source: &str| {
        fs::write(&path, source).expect("write separator source");
        let output = slug().arg(&path).output().expect("run separator source");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (code, stdout, stderr) = run("enum Lines {\n    red\n    green\n}\n\
         enum Commas { red, green, blue }\n\
         enum Mixed {\n    red,\n    green\n    blue,\n}\n\
         val Point = struct {\n    x: num\n    y: num = 2\n}\n\
         val Pair = struct { left: num, right: num }\n\
         val S:schema = struct{\n    name:str,\n    age:num = 1,\n}\n\
         println(Lines.green, Commas.blue, Mixed.blue)\n\
         println(Point {\n  x: 1\n}, Pair { left: 1, right: 2 }, S { name: \"s\" })\n");
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "Lines.green Commas.blue Mixed.blue\n\
         struct {\"x\": 1, \"y\": 2} struct {\"left\": 1, \"right\": 2} struct {\"name\": \"s\", \"age\": 1}\n"
    );

    for (source, message) in [
        (
            "val Point = struct {\n    x: num y: num\n}\n",
            "expected comma or newline between struct fields",
        ),
        (
            "val Point = struct { x: num }\nprintln(Point { x: 1 y: 2 })\n",
            "expected comma between struct fields",
        ),
        (
            "val Point = struct { x, y }\nprintln(Point {\n  x: 1\n  y: 2\n})\n",
            "expected comma between struct fields",
        ),
        (
            "enum Colour { red green }\n",
            "expected comma or newline between enum cases",
        ),
        (
            "enum Colour { red; green }\n",
            "expected comma or newline between enum cases",
        ),
        (
            "val Point = struct { x; y }\n",
            "expected comma or newline between struct fields",
        ),
        ("enum Colour { red,, green }\n", "expected enum case name"),
        (
            "val Point = struct { x,, y }\n",
            "expected struct field name",
        ),
        (
            "val Point = struct { x, y }\nprintln(Point { x: 1,, y: 2 })\n",
            "expected struct field name",
        ),
    ] {
        let (code, _, stderr) = run(source);
        assert_eq!(code, Some(1), "{source}");
        assert!(
            stderr.contains(&format!("parse error: {message}")),
            "{source}: {stderr}"
        );
    }
    fs::remove_file(&path).expect("remove separator source");
}

#[test]
fn double_quoted_strings_escape_backspace_and_form_feed() {
    let path = fixture_path("backspace-form-feed");
    fs::write(
        &path,
        "println(\"\\b\" == \"\\10\", \"\\f\" == \"\\14\", len(\"a\\bb\\fc\"), '\\b' == \"\\\\b\")\n",
    )
    .expect("write escape source");
    let output = slug().arg(&path).output().expect("run escape source");
    fs::remove_file(&path).expect("remove escape source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true true 5 true\n"
    );
}

#[test]
fn unconstrained_parameters_are_dynamic_inside_their_body() {
    let path = fixture_path("unconstrained-parameters");
    let run = |source: &str| {
        fs::write(&path, source).expect("write unconstrained parameter source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run unconstrained parameter source");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (code, stdout, stderr) = run("val identity = fn(value) { value }\n\
         val text:str = identity(\"ok\")\n\
         val repeat = fn():str {\n\
         \x20 val go = fn(i, out) { if (i == 0) { out } else { recur(i - 1, out + \"x\") } }\n\
         \x20 go(3, \"\")\n\
         }\n\
         val collect = fn(i, out) { if (i == 0) { out } else { recur(i - 1, out :+ i) } }\n\
         val items:list = collect(2, [])\n\
         println(text, repeat(), items)\n");
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "ok xxx [2, 1]\n");

    // An explicit any|nil annotation keeps its static meaning.
    let (code, _, stderr) =
        run("val pass = fn(value:any|nil) { value }\nval text:str = pass(\"a\")\n");
    fs::remove_file(&path).expect("remove unconstrained parameter source");
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("semantic error: expected str, got any|nil"),
        "{stderr}"
    );
}

#[test]
fn compatible_parameter_requirements_combine_to_the_narrower_type() {
    let path = fixture_path("parameter-requirements");
    let run = |source: &str| {
        fs::write(&path, source).expect("write parameter requirement source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run parameter requirement source");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    // `e` is a number from `e < 0` and is also passed on to an unconstrained
    // parameter, in either order.
    let (code, stdout, stderr) = run(
        "val pass = fn(flag, value) { if (flag) { value } else { 0 } }\n\
         val before = fn(e) { if (e < 0) { -e } else { pass(true, e) } }\n\
         val after = fn(e) { pass(true, e) * (if (e < 0) { -1 } else { 1 }) }\n\
         println(before(-3), before(2), after(-4))\n",
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "3 2 4\n");

    // The combined requirement is still checked at calls.
    let (code, _, stderr) = run("val pass = fn(value) { value }\n\
         val f = fn(e) { if (e < 0) { 1 } else { pass(e) } }\n\
         f(\"x\")\n");
    assert_eq!(code, Some(1));
    assert!(stderr.contains("semantic error"), "{stderr}");

    // Requirements with no common type still conflict.
    let (code, _, stderr) = run("val g = fn(x) { if (x < 0) { 1 } else { -x } }\n\
         val h = fn(x) { if (x < 0) { 1 } else { x < \"a\" } }\n");
    fs::remove_file(&path).expect("remove parameter requirement source");
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("parameter `x` has incompatible body requirements: num and str"),
        "{stderr}"
    );
}
