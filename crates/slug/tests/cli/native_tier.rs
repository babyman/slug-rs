use super::*;

fn traced_run(source: &str, mode: &str, threshold: Option<&str>) -> (String, String) {
    let path = fixture_path(&format!("native-{mode}-{}", threshold.unwrap_or("default")));
    fs::write(&path, source).expect("write native tier fixture");
    let mut command = slug();
    command
        .env("SLUG_NATIVE", mode)
        .env("SLUG_NATIVE_TRACE", "1")
        .arg(&path);
    match threshold {
        Some(threshold) => command.env("SLUG_NATIVE_THRESHOLD", threshold),
        None => command.env_remove("SLUG_NATIVE_THRESHOLD"),
    };
    let output = command.output().expect("run slug");
    fs::remove_file(&path).expect("remove native tier fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    )
}

const LOOPS: &str = "val step = fn(value) { value + 1 }\n\
val count = fn(remaining, total) {\n\
    if (remaining == 0) { total } else { recur(remaining - 1, step(total)) }\n\
}\n\
val once = fn() { 7 }\n\
val main = fn() { println(count(50, 0), once()) }\n";

#[test]
fn calls_and_recurs_make_a_chunk_hot_once() {
    let (stdout, stderr) = traced_run(LOOPS, "on", Some("40"));
    assert_eq!(stdout, "50 7\n");
    // `step` is called 50 times and `count` entered once and recurred 50
    // times; each is reported once. `once`, `main`, and the module body
    // stay below 40. Functions bound by `val` are anonymous chunks.
    let hot = stderr.lines().collect::<Vec<_>>();
    assert_eq!(hot.len(), 2, "{stderr}");
    assert!(hot.iter().all(|line| line.starts_with("native: hot ")));
}

#[test]
fn all_makes_every_entered_chunk_hot_at_its_first_entry() {
    let (stdout, stderr) = traced_run(LOOPS, "all", None);
    assert_eq!(stdout, "50 7\n");
    // The module body and its four functions, each once.
    let hot = stderr.lines().collect::<Vec<_>>();
    assert_eq!(hot.len(), 5, "{stderr}");
    let mut unique = hot.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 5, "{stderr}");
}

#[test]
fn off_installs_no_tier() {
    let (stdout, stderr) = traced_run(LOOPS, "off", Some("1"));
    assert_eq!(stdout, "50 7\n");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn a_bad_setting_is_a_startup_error() {
    let path = fixture_path("native-bad");
    fs::write(&path, "val main = fn() { 1 }\n").expect("write fixture");
    let output = slug()
        .env("SLUG_NATIVE", "sometimes")
        .arg(&path)
        .output()
        .expect("run slug");
    fs::remove_file(&path).expect("remove fixture");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("SLUG_NATIVE must be on, all, or off"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dump_prints_the_ir_of_each_hot_chunk_and_every_chunk_lifts() {
    let path = fixture_path("native-dump");
    fs::write(&path, LOOPS).expect("write native tier fixture");
    let output = slug()
        .env("SLUG_NATIVE", "all")
        .env("SLUG_NATIVE_TRACE", "1")
        .env("SLUG_NATIVE_DUMP", "1")
        .arg(&path)
        .output()
        .expect("run slug");
    fs::remove_file(&path).expect("remove native tier fixture");
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert_eq!(
        stderr.matches("\nfunction ").count() + usize::from(stderr.starts_with("function ")),
        5,
        "{stderr}"
    );
    assert!(stderr.contains("recur("), "{stderr}");
    assert!(!stderr.contains("cannot lift"), "{stderr}");
}
