use super::*;

#[test]
fn reads_writes_appends_and_explicitly_closes_opaque_file_resources() {
    let root = std::env::temp_dir().join(format!("slug-cli-filesystem-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create filesystem fixture root");
    let input = root.join("input.txt");
    let output = root.join("output.txt");
    let program = root.join("program.slug");
    fs::write(&input, "first\r\n\nlast").expect("write input fixture");
    fs::write(
        &program,
        format!(
            "val fs = import(\"slug.io.fs\")\n\
             val input = fs.openRead(\"{}\")\n\
             defer fs.close(input)\n\
             println(fs.readLine(input))\n\
             println(fs.readLine(input))\n\
             println(fs.readLine(input))\n\
             println(fs.readLine(input))\n\
             val output = fs.openWrite(\"{}\")\n\
             defer fs.close(output)\n\
             println(fs.write(output, \"one\"))\n\
             fs.close(output)\n\
             val appended = fs.openAppend(\"{}\")\n\
             defer fs.close(appended)\n\
             println(fs.write(appended, \" two\"))\n",
            input.display(),
            output.display(),
            output.display(),
        ),
    )
    .expect("write filesystem source");
    let process = slug()
        .arg(&program)
        .output()
        .expect("run filesystem source");
    let output_contents = fs::read_to_string(&output).expect("read output fixture");
    fs::remove_dir_all(root).expect("remove filesystem fixture root");

    assert!(
        process.status.success(),
        "{}",
        String::from_utf8_lossy(&process.stderr)
    );
    assert_eq!(
        String::from_utf8(process.stdout).unwrap(),
        "first\n\nlast\nnil\n3\n4\n"
    );
    assert_eq!(output_contents, "one two");
    assert!(process.stderr.is_empty());
}

#[test]
fn rejects_file_operations_after_explicit_close() {
    let root = std::env::temp_dir().join(format!("slug-cli-closed-file-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create closed-file fixture root");
    let input = root.join("input.txt");
    let program = root.join("program.slug");
    fs::write(&input, "line\n").expect("write input fixture");
    fs::write(
        &program,
        format!(
            "val fs = import(\"slug.io.fs\")\nval file = fs.openRead(\"{}\")\nfs.close(file)\nfs.readLine(file)\n",
            input.display()
        ),
    )
    .expect("write closed-file source");
    let output = slug()
        .arg(&program)
        .output()
        .expect("run closed-file source");
    fs::remove_dir_all(root).expect("remove closed-file fixture root");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("native resource is closed")
    );
}
