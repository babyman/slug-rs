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

#[test]
fn schema_instances_use_schema_identity_not_field_structure() {
    accepts(
        "val Point = struct { x:num, y:num }\n\
         val Size = struct { x:num, y:num }\n\
         val point:struct<Point> = Point { x: 1, y: 2 }\n\
         val size:struct<Size> = Size { x: 3, y: 4 }\n\
         val move = fn(value:struct<Point>):struct<Point> { value copy { x: value.x + 1 } }\n\
         move(point)\n\
         size.x\n",
    );

    for (source, expected) in [
        (
            "val Point = struct { x:num, y:num }\nval Size = struct { x:num, y:num }\nval size:struct<Size> = Point { x: 1, y: 2 }\n",
            "expected struct<Size>, got struct<Point>",
        ),
        (
            "val Point = struct { x:num, y:num }\nval Size = struct { x:num, y:num }\nval consume = fn(value:struct<Point>) { value }\nconsume(Size { x: 1, y: 2 })\n",
            "expected struct<Point>, got struct<Size>",
        ),
        (
            "val Point = struct { x:num, y:num }\nval Size = struct { x:num, y:num }\nval wrong = fn(value:struct<Point>):struct<Size> { value }\nwrong(Point { x: 1, y: 2 })\n",
            "expected struct<Size>, got struct<Point>",
        ),
    ] {
        rejects(source, expected);
    }
}

#[test]
fn aliases_are_transparent_without_replacing_nominal_identity() {
    accepts(
        "type Path = str\n\
         type Filename = Path\n\
         val path:Path = \"notes.txt\"\n\
         val filename:Filename = path\n\
         val display = fn(value:str):Filename { value }\n\
         display(filename)\n\
         resource File\n\
         type Input = File\n\
         foreign read = fn(file:File):num\n\
         val useInput = fn(input:Input) { read(input) }\n",
    );

    rejects(
        "resource File\n\
         resource Socket\n\
         type Input = File\n\
         foreign openSocket = fn():Socket\n\
         val input:Input = openSocket()\n",
        "expected File, got Socket",
    );
}

#[test]
fn callable_paths_preserve_nominal_assignability() {
    accepts(
        "resource File\n\
         resource Socket\n\
         val overload = fn(file:File):File { file }\n\
         val overload = fn(socket:Socket):Socket { socket }\n\
         val fileCallback = fn(file:File):File { file }\n\
         val invoke = fn(callback:fn<File, File>, file:File) { callback(file) }\n\
         val outer = fn(file:File) { overload(file); invoke(fileCallback, file) }\n",
    );

    for (source, expected) in [
        (
            "resource File\nresource Socket\nforeign openSocket = fn():Socket\nval defaulted = fn(file:File = openSocket()) { file }\n",
            "expected File, got Socket",
        ),
        (
            "resource File\nresource Socket\nforeign openSocket = fn():Socket\nval consume = fn(...files:File) { files }\nconsume(openSocket())\n",
            "expected File, got Socket",
        ),
        (
            "resource File\nresource Socket\nval invoke = fn(callback:fn<File, File>, file:File) { callback(file) }\nval outer = fn(file:File) { val socketCallback = fn(socket:Socket):Socket { socket }; invoke(socketCallback, file) }\n",
            "expected File, got Socket",
        ),
        (
            "resource File\nresource Socket\nforeign openSocket = fn():Socket\nval use = fn(file:File) { file }\nval use = fn(socket:Socket) { socket }\nval onlyFile = fn(file:File) { file }\nonlyFile(openSocket())\n",
            "expected File, got Socket",
        ),
    ] {
        rejects(source, expected);
    }
}
