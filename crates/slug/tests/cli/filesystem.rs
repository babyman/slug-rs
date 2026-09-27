use super::*;

#[test]
fn requires_an_installed_filesystem_clutch() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-filesystem-unavailable-{}",
        std::process::id()
    ));
    let home = root.join("home");
    let program = root.join("program.slug");
    fs::create_dir_all(&home).expect("create empty SLUG_HOME");
    fs::write(&program, "val fs = import(\"slug.io.fs\")\n")
        .expect("write filesystem-importing source");

    let output = slug()
        .arg(&program)
        .env("SLUG_HOME", &home)
        .env_remove("SLUG_FIXTURE_LIBRARY_ROOT")
        .output()
        .expect("run source without filesystem clutch");
    fs::remove_dir_all(root).expect("remove unavailable filesystem fixture root");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("module `slug.io.fs` was not found")
    );
}

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
             val input:fs.File = fs.openRead(\"{}\")\n\
             defer fs.close(input)\n\
             println(\"resource\")\n\
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
        "resource\nfirst\n\nlast\nnil\n3\n4\n"
    );
    assert_eq!(output_contents, "one two");
    assert!(process.stderr.is_empty());
}

#[test]
fn resource_annotations_check_foreign_results_and_arguments() {
    let root = std::env::temp_dir().join(format!("slug-cli-resource-type-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create resource-type fixture root");
    let input = root.join("input.txt");
    let program = root.join("program.slug");
    fs::write(&input, "line\n").expect("write input fixture");
    fs::write(
        &program,
        format!(
            "val fs = import(\"slug.io.fs\")\nval file = fs.openRead(\"{}\")\nfs.readLine(\"not a file\")\n",
            input.display()
        ),
    )
    .expect("write resource-type source");
    let output = slug()
        .arg(&program)
        .output()
        .expect("type-check resource source");
    fs::remove_dir_all(root).expect("remove resource-type fixture root");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("expected File, got str")
    );
}

#[test]
fn preserves_nominal_file_types_from_foreign_results() {
    let root = std::env::temp_dir().join(format!("slug-cli-file-inference-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create file-inference fixture root");
    let input = root.join("input.txt");
    let program = root.join("program.slug");
    fs::write(&input, "line\n").expect("write file-inference input");
    fs::write(
        &program,
        format!(
            "val fs = import(\"slug.io.fs\")\n\
             val file:fs.File = fs.openRead(\"{}\")\n\
             defer fs.close(file)\n\
             val files:list<fs.File> = [file, file]\n\
             println(len(files))\n",
            input.display()
        ),
    )
    .expect("write file-inference source");
    let output = slug()
        .arg(&program)
        .output()
        .expect("run file-inference source");
    fs::remove_dir_all(root).expect("remove file-inference fixture root");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "2\n");
}

#[test]
fn infers_conditional_file_results_inside_lists() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-conditional-file-inference-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create conditional file fixture root");
    let input = root.join("input.txt");
    let program = root.join("program.slug");
    fs::write(&input, "line\n").expect("write conditional file input");
    fs::write(
        &program,
        format!(
            "val fs = import(\"slug.io.fs\")\n\
             val files:list<fs.File> = [if (true) {{ fs.openRead(\"{}\") }} else {{ fs.openRead(\"{}\") }}]\n\
             defer fs.close(files[0])\n\
             println(len(files))\n",
            input.display(),
            input.display(),
        ),
    )
    .expect("write conditional file inference source");
    let output = slug()
        .arg(&program)
        .output()
        .expect("run conditional file inference source");
    fs::remove_dir_all(root).expect("remove conditional file fixture root");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n");
}

#[test]
fn match_constraints_check_exact_nominal_file_handles() {
    let root = std::env::temp_dir().join(format!("slug-cli-resource-match-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create resource-match fixture root");
    let input = root.join("input.txt");
    let program = root.join("program.slug");
    fs::write(&input, "line\n").expect("write resource-match input");
    fs::write(
        &program,
        format!(
            "val fs = import(\"slug.io.fs\")\n\
             val file = fs.openRead(\"{}\")\n\
             defer fs.close(file)\n\
             val classify = fn(value) match {{ _:fs.File => \"file\"; _ => \"other\" }}\n\
             println(classify(file), classify(\"not a file\"))\n",
            input.display(),
        ),
    )
    .expect("write resource-match source");
    let output = slug()
        .arg(&program)
        .output()
        .expect("run resource-match source");
    fs::remove_dir_all(root).expect("remove resource-match fixture root");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "file other\n");
}

#[test]
fn rejects_unknown_imported_resource_match_constraints() {
    let path = fixture_path("unknown-resource-match-type");
    fs::write(
        &path,
        "val fs = import(\"slug.io.fs\")\nmatch nil { _:fs.Missing => true }\n",
    )
    .expect("write unknown resource-match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run unknown resource-match source");
    fs::remove_file(path).expect("remove unknown resource-match source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unknown type `fs.Missing`")
    );
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
