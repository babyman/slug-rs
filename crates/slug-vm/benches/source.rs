use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

use serde::Serialize;

const WARMUP_RUNS: usize = 3;
const DEFAULT_SAMPLES: usize = 15;
const WORKLOADS: &[Workload] = &[
    Workload {
        name: "function-call",
        expected_stdout: "100000\n",
    },
    Workload {
        name: "n-body",
        expected_stdout: "ok\n",
    },
    Workload {
        name: "typed-n-body",
        expected_stdout: "ok\n",
    },
    Workload {
        name: "spectral-norm",
        expected_stdout: "ok\n",
    },
    Workload {
        name: "typed-spectral-norm",
        expected_stdout: "ok\n",
    },
    Workload {
        name: "binary-trees",
        expected_stdout: "65535\n",
    },
    Workload {
        name: "fannkuch-redux",
        expected_stdout: "-7 7\n",
    },
];

struct Workload {
    name: &'static str,
    expected_stdout: &'static str,
}

#[derive(Serialize)]
struct BenchmarkReport<'a> {
    schema_version: u8,
    revision: String,
    worktree_dirty: bool,
    samples: usize,
    warmup_runs: usize,
    slug: Runtime<'a>,
    python: Runtime<'a>,
    workloads: Vec<WorkloadReport<'a>>,
}

#[derive(Serialize)]
struct Runtime<'a> {
    executable: &'a str,
    version: String,
}

#[derive(Serialize)]
struct WorkloadReport<'a> {
    name: &'a str,
    slug_median_ns: u128,
    python_median_ns: u128,
    slug_over_python: f64,
}

fn main() {
    let json = env::args().skip(1).any(|argument| argument == "--json");
    let samples = sample_count();
    let root = workspace_root();
    let slug = slug_executable(&root);
    let python = python_executable();
    let source_root = root.join("crates/slug-vm/benches/source");

    validate_workloads(&slug, &python, &source_root);
    for workload in WORKLOADS {
        warm_up(&slug, &source_path(&source_root, "slug", workload));
        warm_up(&python, &source_path(&source_root, "python", workload));
    }

    let workloads = WORKLOADS
        .iter()
        .map(|workload| benchmark_workload(&slug, &python, &source_root, workload, samples))
        .collect::<Vec<_>>();
    let report = BenchmarkReport {
        schema_version: 1,
        revision: git_revision(&root),
        worktree_dirty: git_worktree_dirty(&root),
        samples,
        warmup_runs: WARMUP_RUNS,
        slug: Runtime {
            executable: slug.to_str().expect("Slug executable path is UTF-8"),
            version: command_version(&slug, "--version"),
        },
        python: Runtime {
            executable: python.to_str().expect("Python executable path is UTF-8"),
            version: command_version(&python, "--version"),
        },
        workloads,
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("benchmark report is serializable")
        );
    } else {
        print_report(&report);
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("slug-vm is nested below the workspace root")
        .to_path_buf()
}

fn slug_executable(root: &Path) -> PathBuf {
    env::var_os("SLUG_BENCH_SLUG").map_or_else(|| root.join("target/release/slug"), PathBuf::from)
}

fn python_executable() -> PathBuf {
    env::var_os("SLUG_BENCH_PYTHON")
        .map_or_else(|| PathBuf::from("/usr/bin/python3"), PathBuf::from)
}

fn sample_count() -> usize {
    env::var("SLUG_BENCH_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|count: &usize| *count > 0)
        .unwrap_or(DEFAULT_SAMPLES)
}

fn source_path(root: &Path, runtime: &str, workload: &Workload) -> PathBuf {
    let extension = if runtime == "slug" { "slug" } else { "py" };
    root.join(runtime)
        .join(workload.name)
        .with_extension(extension)
}

fn validate_workloads(slug: &Path, python: &Path, source_root: &Path) {
    for workload in WORKLOADS {
        for (runtime, executable) in [("Slug", slug), ("Python", python)] {
            let source = source_path(
                source_root,
                if runtime == "Slug" { "slug" } else { "python" },
                workload,
            );
            let output = run(executable, &source);
            assert!(
                output.status.success(),
                "{runtime} {} failed:\n{}",
                workload.name,
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                output.stdout,
                workload.expected_stdout.as_bytes(),
                "{runtime} {} produced unexpected output",
                workload.name
            );
        }
    }
}

fn warm_up(executable: &Path, source: &Path) {
    for _ in 0..WARMUP_RUNS {
        let output = run(executable, source);
        assert!(output.status.success(), "warmup execution failed");
    }
}

fn benchmark_workload<'a>(
    slug: &Path,
    python: &Path,
    source_root: &Path,
    workload: &'a Workload,
    samples: usize,
) -> WorkloadReport<'a> {
    let slug_median = median(
        (0..samples)
            .map(|_| measure(slug, &source_path(source_root, "slug", workload)))
            .collect(),
    );
    let python_median = median(
        (0..samples)
            .map(|_| measure(python, &source_path(source_root, "python", workload)))
            .collect(),
    );
    WorkloadReport {
        name: workload.name,
        slug_median_ns: slug_median.as_nanos(),
        python_median_ns: python_median.as_nanos(),
        slug_over_python: slug_median.as_secs_f64() / python_median.as_secs_f64(),
    }
}

fn measure(executable: &Path, source: &Path) -> Duration {
    let started = Instant::now();
    let output = run(executable, source);
    assert!(
        output.status.success(),
        "benchmark execution failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    started.elapsed()
}

fn run(executable: &Path, source: &Path) -> Output {
    Command::new(executable)
        .arg(source)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", executable.display()))
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn command_version(executable: &Path, argument: &str) -> String {
    let output = Command::new(executable)
        .arg(argument)
        .output()
        .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", executable.display()));
    assert!(output.status.success(), "cannot inspect runtime version");
    let version = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(version).trim().to_owned()
}

fn print_report(report: &BenchmarkReport<'_>) {
    println!(
        "Slug source benchmarks: {} samples, {} warmups, revision {}{}",
        report.samples,
        report.warmup_runs,
        report.revision,
        if report.worktree_dirty {
            " (dirty)"
        } else {
            ""
        },
    );
    println!("Slug: {} ({})", report.slug.executable, report.slug.version);
    println!(
        "Python: {} ({})",
        report.python.executable, report.python.version
    );
    println!("\nworkload                 Slug median   Python median  Slug / Python");
    for workload in &report.workloads {
        println!(
            "{:<24} {:>11} ms {:>11} ms {:>11.2}x",
            workload.name,
            format_millis(workload.slug_median_ns),
            format_millis(workload.python_median_ns),
            workload.slug_over_python,
        );
    }
}

fn format_millis(nanoseconds: u128) -> String {
    let milliseconds = nanoseconds / 1_000_000;
    let fractional = (nanoseconds % 1_000_000) / 1_000;
    format!("{milliseconds}.{fractional:03}")
}

fn git_revision(root: &Path) -> String {
    Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || "unknown".to_owned(),
            |output| String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        )
}

fn git_worktree_dirty(root: &Path) -> bool {
    Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_some_and(|output| output.status.success() && !output.stdout.is_empty())
}
