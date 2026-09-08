use slug_vm::compile;

fn accepts(source: &str) {
    compile("nominal-test.slug", source).unwrap_or_else(|error| panic!("{error}\n{source}"));
}

fn rejects(source: &str, expected: &str) {
    let error = compile("nominal-test.slug", source).expect_err("source must be rejected");
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn resource_assignability_uses_declaration_identity() {
    accepts(
        "resource File;\n\
         resource Socket;\n\
         foreign read = fn(file:File):num;\n\
         val useFile = fn(file:File) { file }\n\
         val useSocket = fn(socket:Socket) { socket }\n",
    );

    for (source, expected) in [
        (
            "resource File\nresource Socket\nforeign open = fn():File\nval value:Socket = open()\n",
            "expected Socket, got File",
        ),
        (
            "resource File\nresource Socket\nval set = fn(value:File, socket:Socket) { var result:File = value; result = socket }\n",
            "expected File, got Socket",
        ),
        (
            "resource File\nresource Socket\nforeign open = fn():Socket\nval read = fn(value:File) { value }\nread(open())\n",
            "expected File, got Socket",
        ),
        (
            "resource File\nresource Socket\nforeign open = fn():File\nval wrong = fn(value:File):Socket { value }\nwrong(open())\n",
            "expected Socket, got File",
        ),
        (
            "resource File\nresource Socket\nforeign open = fn():Socket\nforeign read = fn(value:File):num\nread(open())\n",
            "expected File, got Socket",
        ),
    ] {
        rejects(source, expected);
    }
}

#[test]
fn enum_assignability_uses_declaration_identity() {
    accepts(
        "enum ReadMode { Text, Binary }\n\
         enum WriteMode { Text, Binary }\n\
         val read = fn(mode:ReadMode):ReadMode { mode }\n\
         val write = fn(mode:WriteMode):WriteMode { mode }\n\
         val text:ReadMode = ReadMode.Text\n\
         read(text)\n\
         write(WriteMode.Binary)\n",
    );

    for (source, expected) in [
        (
            "enum ReadMode { Text, Binary }\nenum WriteMode { Text, Binary }\nval mode:ReadMode = WriteMode.Text\n",
            "expected ReadMode, got WriteMode",
        ),
        (
            "enum ReadMode { Text, Binary }\nenum WriteMode { Text, Binary }\nval read = fn(mode:ReadMode) { mode }\nread(WriteMode.Text)\n",
            "expected ReadMode, got WriteMode",
        ),
        (
            "enum ReadMode { Text, Binary }\nenum WriteMode { Text, Binary }\nval wrong = fn(mode:ReadMode):WriteMode { mode }\nwrong(ReadMode.Text)\n",
            "expected WriteMode, got ReadMode",
        ),
        (
            "enum ReadMode { Text, Binary }\nenum WriteMode { Text, Binary }\nval mode:ReadMode|nil = WriteMode.Text\n",
            "expected ReadMode|nil, got WriteMode",
        ),
        (
            "enum ReadMode { Text, Binary }\nenum WriteMode { Text, Binary }\nval subject:ReadMode = ReadMode.Text\nmatch subject { WriteMode.Text => 1; _ => 0 }\n",
            "match case cannot match remaining enum cases",
        ),
    ] {
        rejects(source, expected);
    }
}
