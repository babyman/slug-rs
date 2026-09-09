use super::*;

#[test]
fn infers_scalar_literal_types_through_bindings() {
    let path = fixture_path("scalar-literal-inference");
    fs::write(
        &path,
        "val absent = nil\n\
         val enabled = true\n\
         val count = 42\n\
         val label = \"Slug\"\n\
         val data = 0x\"534c5547\"\n\
         val accept_nil = fn(value:nil) { value }\n\
         val accept_bool = fn(value:bool) { value }\n\
         val accept_num = fn(value:num) { value }\n\
         val accept_str = fn(value:str) { value }\n\
         val accept_bytes = fn(value:bytes) { value }\n\
         println(accept_nil(absent), accept_bool(enabled), accept_num(count), accept_str(label), accept_bytes(data))\n",
    )
    .expect("write scalar literal inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run scalar literal inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "nil true 42 Slug 0x\"534c5547\"\n"
    );

    for (literal, annotation, expected) in [
        ("nil", "str", "expected str, got nil"),
        ("true", "num", "expected num, got bool"),
        ("42", "str", "expected str, got num"),
        ("\"Slug\"", "num", "expected num, got str"),
        ("0x\"534c5547\"", "str", "expected str, got bytes"),
    ] {
        fs::write(
            &path,
            format!("val value = {literal}\nval invalid:{annotation} = value\n"),
        )
        .expect("write incompatible scalar binding source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run incompatible scalar binding source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
        assert!(
            stderr.starts_with(&format!("slug: semantic error: {expected}")),
            "{stderr}"
        );
    }
    fs::remove_file(path).expect("remove scalar literal inference source");
}

#[test]
fn preserves_inferred_types_across_transitive_bindings() {
    let path = fixture_path("transitive-binding-inference");
    fs::write(
        &path,
        "val a = \"slug\"\nval b = a\nval c = b\nprintln(c)\n",
    )
    .expect("write transitive binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run transitive binding source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "slug\n");

    fs::write(&path, "val a = \"slug\"\nval b = a\nval c = b\nc - 1\n")
        .expect("write invalid transitive binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid transitive binding source");
    fs::remove_file(path).expect("remove transitive binding source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got str"),
        "{stderr}"
    );
}

#[test]
fn exposes_annotated_bindings_at_their_declared_type() {
    let path = fixture_path("annotated-binding-inference");
    fs::write(
        &path,
        "val value:num|nil = 10\n\
         val display = fn(item:num|nil) { item }\n\
         println(display(value))\n",
    )
    .expect("write annotated binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run annotated binding source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "10\n");

    fs::write(&path, "val value:num|nil = 10\nval invalid:num = value\n")
        .expect("write narrowed annotated binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run narrowed annotated binding source");
    fs::remove_file(path).expect("remove annotated binding source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got num|nil"),
        "{stderr}"
    );
}

#[test]
fn fixes_inferred_mutable_binding_types_at_initialization() {
    let path = fixture_path("mutable-binding-inference");
    fs::write(&path, "var value = 1\nvalue = 2\nprintln(value)\n")
        .expect("write compatible mutable binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run compatible mutable binding source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "2\n");

    fs::write(&path, "var value = 1\nvalue = \"slug\"\n")
        .expect("write incompatible mutable binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run incompatible mutable binding source");
    fs::remove_file(path).expect("remove mutable binding source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got str"),
        "{stderr}"
    );
}

#[test]
fn infers_and_checks_prefix_operator_results() {
    let path = fixture_path("prefix-inference");
    fs::write(
        &path,
        "val negate = fn(value) { -value }\n\
         println(-2, !nil, ~0x\"00\", negate(3))\n",
    )
    .expect("write prefix inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run prefix inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "-2 true 0x\"ff\" -3\n"
    );

    for (source, expected) in [
        ("val value = \"slug\"\n-value\n", "expected num, got str"),
        (
            "val value = true\n~value\n",
            "operator `~` does not accept bool",
        ),
    ] {
        fs::write(&path, source).expect("write invalid prefix inference source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid prefix inference source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
        assert!(
            stderr.starts_with(&format!("slug: semantic error: {expected}")),
            "{stderr}"
        );
    }
    fs::remove_file(path).expect("remove prefix inference source");
}

#[test]
fn infers_and_checks_binary_operator_results() {
    let path = fixture_path("binary-inference");
    fs::write(
        &path,
        "val subtract = fn(left, right) { left - right }\n\
         println(3 + 4, 3 < 4, 3 == 4, \"Slug\" + 1, true && false, subtract(5, 2))\n",
    )
    .expect("write binary inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run binary inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "7 true false Slug1 false 3\n"
    );

    fs::write(&path, "val value = \"slug\"\nvalue - 1\n")
        .expect("write invalid binary inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid binary inference source");
    fs::remove_file(path).expect("remove binary inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got str"),
        "{stderr}"
    );
}

#[test]
fn infers_block_results_without_leaking_block_bindings() {
    let path = fixture_path("block-result-inference");
    fs::write(
        &path,
        "val doubled:num = { val value = 10\nvalue * 2 }\n\
         val empty:nil = if (true) {}\n\
         println(doubled, empty)\n",
    )
    .expect("write block result inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run block result inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "20 nil\n");

    fs::write(&path, "val result = { val local = 1\nlocal }\nlocal\n")
        .expect("write escaped block binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run escaped block binding source");
    fs::remove_file(path).expect("remove block result inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: runtime error: unknown name `local`"),
        "{stderr}"
    );
}

#[test]
fn return_payloads_infer_function_results_without_falling_through() {
    let path = fixture_path("return-flow-inference");
    fs::write(
        &path,
        "val describe = fn():str { return \"returned\"\n42 }\nprintln(describe())\n",
    )
    .expect("write return-flow inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run return-flow inference source");
    fs::remove_file(path).expect("remove return-flow inference source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "returned\n");
}

#[test]
fn infers_normalized_if_result_types() {
    let path = fixture_path("if-result-inference");
    fs::write(
        &path,
        "val same:num = if (true) { 1 } else { 2 }\n\
         val different:num|str = if (false) { 1 } else { \"one\" }\n\
         val optional:num|nil = if (false) { 1 }\n\
         println(same, different, optional)\n",
    )
    .expect("write if result inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run if result inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1 one nil\n");

    fs::write(
        &path,
        "val invalid:num = if (false) { 1 } else { \"one\" }\n",
    )
    .expect("write incompatible if result source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run incompatible if result source");
    fs::remove_file(path).expect("remove if result inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got num|str"),
        "{stderr}"
    );
}

#[test]
fn infers_list_literal_element_types() {
    let path = fixture_path("list-literal-inference");
    fs::write(
        &path,
        "val numbers:list<num> = [1, 2, 3]\n\
         val mixed:list<num|str> = [1, \"two\"]\n\
         val nested:list<list<num> > = [[1], [2]]\n\
         val empty:list = []\n\
         println(numbers, mixed, nested, empty)\n",
    )
    .expect("write list literal inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run list literal inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[1, 2, 3] [1, \"two\"] [[1], [2]] []\n"
    );

    fs::write(&path, "val invalid:list<num> = [1, \"two\"]\n")
        .expect("write incompatible list literal source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run incompatible list literal source");
    fs::remove_file(path).expect("remove list literal inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected list<num>, got list<num|str>"),
        "{stderr}"
    );
}

#[test]
fn infers_list_spread_element_types_without_conflating_unknown_and_any() {
    let path = fixture_path("list-spread-inference");
    fs::write(
        &path,
        "val words:list<str> = [\"two\"]\n\
         val mixed:list<num|str> = [1, ...words]\n\
         val make_any = fn(value:any) { [value] }\n\
         val values:list<any> = make_any(\"value\")\n\
         val dynamic:list<any> = [1, ...values]\n\
         val collect = fn(value) { [1, ...value] }\n\
         val unknown:list = collect([2])\n\
         val first:num = unknown[0]\n\
         println(mixed, dynamic, unknown, first)\n",
    )
    .expect("write list spread inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run list spread inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[1, \"two\"] [1, \"value\"] [1, 2] 1\n"
    );
    fs::remove_file(path).expect("remove list spread inference source");
}

#[test]
fn infers_map_literal_key_and_value_types_independently() {
    let path = fixture_path("map-literal-inference");
    fs::write(
        &path,
        "val scores:map<str, num> = {\"one\": 1, \"two\": 2}\n\
         val mixed:map<num|str, num|str> = {[1]: \"one\", \"two\": 2}\n\
         val empty:map = {}\n\
         println(scores, mixed, empty)\n",
    )
    .expect("write map literal inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run map literal inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"one\": 1, \"two\": 2} {1: \"one\", \"two\": 2} {}\n"
    );

    fs::write(&path, "val invalid:map<str, num> = {\"one\": \"one\"}\n")
        .expect("write incompatible map literal source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run incompatible map literal source");
    fs::remove_file(path).expect("remove map literal inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected map<str, num>, got map<str, str>"),
        "{stderr}"
    );
}

#[test]
fn preserves_spawn_result_types_through_task_await() {
    let path = fixture_path("task-result-inference");
    fs::write(
        &path,
        "val channel = import(\"slug.channel\")\n\
         val task:task<num> = spawn { 42 }\n\
         val result:num = channel.await(task)\n\
         println(result)\n",
    )
    .expect("write task result inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run task result inference source");
    fs::remove_file(path).expect("remove task result inference source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42\n");
}

#[test]
fn preserves_explicit_and_contextual_channel_element_types() {
    let path = fixture_path("channel-element-inference");
    fs::write(
        &path,
        "val { await, chan, recv, send } = import(\"slug.channel\")\n\
         val inbox = chan<num>()\n\
         val sender = spawn { send(inbox, 1) }\n\
         val received:num|nil = recv(inbox)\n\
         await(sender)\n\
         val contextual:chan<num> = chan()\n\
         println(received)\n",
    )
    .expect("write typed channel source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run typed channel source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n");

    for source in [
        "val { chan, send } = import(\"slug.channel\")\nval inbox = chan<num>()\nsend(inbox, \"wrong\")\n",
        "val { chan, send } = import(\"slug.channel\")\nval inbox:chan<num> = chan()\nsend(inbox, \"wrong\")\n",
    ] {
        fs::write(&path, source).expect("write invalid typed channel source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid typed channel source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
        assert!(
            stderr.starts_with("slug: semantic error: expected num, got str"),
            "{stderr}"
        );
    }
    fs::remove_file(path).expect("remove typed channel source");
}

#[test]
fn infers_normalized_select_handler_results() {
    let path = fixture_path("select-result-inference");
    fs::write(
        &path,
        "val same:num = select { _ /> fn(_) { 1 } }\n\
         val different:num|str = select {\n\
           _ /> fn(_) { 1 }\n\
           after 1 /> fn(_) { \"one\" }\n\
         }\n\
         println(same, different)\n",
    )
    .expect("write select result inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run select result inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1 1\n");

    fs::write(
        &path,
        "val invalid:num = select { _ /> fn(_) { 1 }; after 1 /> fn(_) { \"one\" } }\n",
    )
    .expect("write incompatible select result source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run incompatible select result source");
    fs::remove_file(path).expect("remove select result inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got num|str"),
        "{stderr}"
    );
}

#[test]
fn infers_normalized_match_case_results() {
    let path = fixture_path("match-result-inference");
    fs::write(
        &path,
        "val same:num = match [1] { [item] => item; _ => 2 }\n\
         val different:num|str = match 1 { 1 => 1; _ => \"other\" }\n\
         println(same, different)\n",
    )
    .expect("write match result inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run match result inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1 1\n");

    fs::write(
        &path,
        "val invalid:num = match 1 { 1 => 1; _ => \"other\" }\n",
    )
    .expect("write incompatible match result source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run incompatible match result source");
    fs::remove_file(path).expect("remove match result inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got num|str"),
        "{stderr}"
    );
}

#[test]
fn excludes_non_returning_branches_from_inferred_results() {
    let path = fixture_path("non-returning-inference");
    fs::write(
        &path,
        "val thrown:num = if (true) { 10 } else { throw \"failed\" }\n\
         val retry = fn(flag:bool):num { if (flag) { 20 } else { recur(true) } }\n\
         println(thrown, retry(false))\n",
    )
    .expect("write non-returning inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-returning inference source");
    fs::remove_file(path).expect("remove non-returning inference source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "10 20\n");
}

#[test]
fn preserves_nested_known_call_results() {
    let path = fixture_path("nested-call-inference");
    fs::write(
        &path,
        "val source = fn() { 10 }\n\
         val transform = fn(value:num) { value + 1 }\n\
         val result:num = transform(source())\n\
         println(result)\n",
    )
    .expect("write nested call inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nested call inference source");
    fs::remove_file(path).expect("remove nested call inference source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "11\n");
}

#[test]
fn infers_known_index_result_types() {
    let path = fixture_path("index-inference");
    fs::write(
        &path,
        "val list_value:num = [1][0]\n\
         val map_value:num|nil = {\"one\": 1}[\"one\"]\n\
         val character:str = \"Slug\"[0]\n\
         val byte:num = 0x\"53\"[0]\n\
         println(list_value, map_value, character, byte)\n",
    )
    .expect("write index inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run index inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1 1 S 83\n");

    fs::write(&path, "val invalid = [1][\"one\"]\n").expect("write invalid index inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid index inference source");
    fs::remove_file(path).expect("remove index inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected num, got str"),
        "{stderr}"
    );
}

#[test]
fn preserves_collection_types_through_slices() {
    let path = fixture_path("slice-inference");
    fs::write(
        &path,
        "val numbers:list<num> = [1, 2, 3][1:]\n\
         val text:str = \"Slug\"[1:3]\n\
         val data:bytes = 0x\"534c5547\"[1:3]\n\
         println(numbers, text, data)\n",
    )
    .expect("write slice inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run slice inference source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[2, 3] lu 0x\"4c55\"\n"
    );

    fs::write(&path, "val invalid = 1[0:1]\n").expect("write invalid slice inference source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid slice inference source");
    fs::remove_file(path).expect("remove slice inference source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: expected list, got num"),
        "{stderr}"
    );
}

#[test]
fn reports_diagnostics_from_inferred_scalar_and_nominal_types() {
    let path = fixture_path("nominal-resource-types");
    fs::write(&path, "val value = \"hello\"\nvalue - 1\n")
        .expect("write inferred scalar diagnostic source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run inferred scalar diagnostic source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: semantic error: expected num, got str"));

    fs::write(
        &path,
        "resource File\nresource Socket\nval read = fn(file:File):num { 1 }\nval invalid = fn(socket:Socket) { val inferred = socket; read(inferred) }\n",
    )
    .expect("write nominal resource source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nominal resource source");
    fs::remove_file(&path).expect("remove nominal resource source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("expected File, got Socket"), "{stderr}");
}

#[test]
fn rejects_duplicate_resource_declarations() {
    let path = fixture_path("duplicate-resource-type");
    fs::write(&path, "resource File\nresource File\n").expect("write duplicate resources");
    let output = slug().arg(&path).output().expect("run duplicate resources");
    fs::remove_file(&path).expect("remove duplicate resources");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("duplicate resource type `File`")
    );
}

#[test]
fn resolves_resource_types_only_through_imported_module_type_namespaces() {
    let path = fixture_path("qualified-resource-type-paths");
    for (source, expected) in [
        (
            "val file:missing.File = nil\n",
            "unknown module binding `missing`",
        ),
        (
            "val fs = 1\nval file:fs.File = nil\n",
            "type prefix `fs` is not a module binding",
        ),
        (
            "val fs = import(\"slug.io.fs\")\nval file:fs.Missing = nil\n",
            "unknown type `fs.Missing`",
        ),
    ] {
        fs::write(&path, source).expect("write invalid qualified resource type");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid qualified resource type");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .contains(expected)
        );
    }
    fs::remove_file(path).expect("remove qualified resource type fixture");
}

#[test]
fn map_all_imports_exported_resource_types_into_the_local_type_namespace() {
    let path = fixture_path("map-all-resource-types");
    fs::write(
        &path,
        "val {*} = import(\"slug.io.fs\")\nval file:File = nil\nprintln(file)\n",
    )
    .expect("write map-all resource type source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run map-all resource type source");
    fs::remove_file(path).expect("remove map-all resource type source");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected File, got nil")
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn accepts_annotations_and_checks_provable_mismatches_on_request() {
    let path = fixture_path("type-annotations");
    fs::write(
        &path,
        "val label:str|nil = \"ready\"\nval User = struct { name:str = \"Slug\" }\nval double = fn<T>(value:num):num { value * 2 }\nprintln(label, double(2), User {}.name)\n",
    )
    .expect("write annotated source");
    let output = slug().arg(&path).output().expect("run annotated source");
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
    let output = slug().arg(&path).output().expect("run generic call source");
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
    let output = slug().arg(&path).output().expect("run mismatched return");
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

    let source = "val choose:fn<str, str> = if (true) { fn(value:str):str { value } } else { fn(value:str):str { value } }\nchoose(1)\n";
    fs::write(&path, source).expect("write invalid function value call source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid function value call source");
    fs::remove_file(&path).expect("remove function value call source");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("slug: semantic error: expected str, got num"),
        "{stderr}"
    );
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
    let output = slug().arg(&path).output().expect("run nil-to-any source");
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
        "slug: parse error: documentation blocks and tags must prefix a val, var, foreign, resource, or enum declaration",
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
            "slug: parse error: documentation blocks and tags must prefix a val, var, foreign, resource, or enum declaration",
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
