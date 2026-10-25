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
        held_out: false,
        expected_stdout: "100000\n",
        python: true,
    },
    Workload {
        name: "n-body",
        held_out: false,
        expected_stdout: "ok\n",
        python: true,
    },
    Workload {
        name: "typed-n-body",
        held_out: false,
        expected_stdout: "ok\n",
        python: true,
    },
    Workload {
        name: "spectral-norm",
        held_out: false,
        expected_stdout: "ok\n",
        python: true,
    },
    Workload {
        name: "typed-spectral-norm",
        held_out: false,
        expected_stdout: "ok\n",
        python: true,
    },
    Workload {
        name: "binary-trees",
        held_out: false,
        expected_stdout: "65535\n",
        python: true,
    },
    Workload {
        name: "fannkuch-redux",
        held_out: false,
        expected_stdout: "-488 16\n",
        python: true,
    },
    Workload {
        name: "shared-append",
        held_out: false,
        expected_stdout: "32004000\n",
        python: true,
    },
    Workload {
        name: "higher-order",
        held_out: false,
        expected_stdout: "15335944\n",
        python: true,
    },
    Workload {
        name: "fib",
        held_out: false,
        expected_stdout: "196418\n",
        python: true,
    },
    Workload {
        name: "defer-errors",
        held_out: false,
        expected_stdout: "187500\n",
        python: true,
    },
    Workload {
        name: "word-frequency",
        held_out: false,
        expected_stdout: "509 50000 99\n",
        python: true,
    },
    Workload {
        name: "string-building",
        held_out: false,
        expected_stdout: "378894 20000 item 20000 i\n",
        python: true,
    },
    Workload {
        name: "expression-evaluator",
        held_out: true,
        expected_stdout: "900 1838462571\n",
        python: true,
    },
    Workload {
        name: "graph-search",
        held_out: true,
        expected_stdout: "3000 22328 10\n",
        python: true,
    },
    Workload {
        name: "text-wrap",
        held_out: true,
        expected_stdout: "210000 3750 60000\n",
        python: true,
    },
    Workload {
        name: "invaders-game",
        held_out: false,
        expected_stdout: "1180 2 1 252475 4116782444\n",
        python: false,
    },
    Workload {
        name: "crawl-game",
        held_out: false,
        expected_stdout: "26 1 12 1061522789\n",
        python: false,
    },
    Workload {
        name: "http-server",
        held_out: false,
        expected_stdout: "4228 619800\n",
        python: false,
    },
    Workload {
        name: "tiny-interpreter",
        held_out: true,
        expected_stdout: "1597\n2001\n[2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59]\n[7, 466, 994]\n30\n",
        python: false,
    },
];

struct Workload {
    name: &'static str,
    /// Checked only for regressions; never used to choose or tune an
    /// optimization.
    held_out: bool,
    expected_stdout: &'static str,
    /// Whether a `CPython` twin exists. Real programs (the eval projects'
    /// references) are Slug only: they are timed but have no ratio.
    python: bool,
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
    held_out: bool,
    slug_median_ns: u128,
    python_median_ns: Option<u128>,
    slug_over_python: Option<f64>,
}

fn main() {
    let json = env::args().skip(1).any(|argument| argument == "--json");
    let samples = sample_count();
    let root = workspace_root();
    let slug = slug_executable(&root);
    let python = python_executable();
    let source_root = root.join("crates/slug-frontend/benches/source");

    validate_workloads(&slug, &python, &source_root);
    for workload in WORKLOADS {
        warm_up(&slug, &source_path(&source_root, "slug", workload));
        if workload.python {
            warm_up(&python, &source_path(&source_root, "python", workload));
        }
    }

    let workloads = WORKLOADS
        .iter()
        .map(|workload| benchmark_workload(&slug, &python, &source_root, workload, samples))
        .collect::<Vec<_>>();
    let report = BenchmarkReport {
        schema_version: 3,
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
        let runtimes: &[(&str, &Path)] = if workload.python {
            &[("Slug", slug), ("Python", python)]
        } else {
            &[("Slug", slug)]
        };
        for &(runtime, executable) in runtimes {
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
    let python_median = workload.python.then(|| {
        median(
            (0..samples)
                .map(|_| measure(python, &source_path(source_root, "python", workload)))
                .collect(),
        )
    });
    WorkloadReport {
        name: workload.name,
        held_out: workload.held_out,
        slug_median_ns: slug_median.as_nanos(),
        python_median_ns: python_median.map(|median| median.as_nanos()),
        slug_over_python: python_median
            .map(|median| slug_median.as_secs_f64() / median.as_secs_f64()),
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
    let mut command = Command::new(executable);
    // `http-server` imports the staged slug.net.tcp clutch from the checkout.
    if env::var_os("SLUG_HOME").is_none() {
        command.env("SLUG_HOME", workspace_root());
    }
    command
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
        let (python, ratio) = match (workload.python_median_ns, workload.slug_over_python) {
            (Some(median), Some(ratio)) => (format_millis(median), format!("{ratio:.2}x")),
            _ => ("-".to_owned(), "-".to_owned()),
        };
        println!(
            "{:<24} {:>11} ms {:>11} ms {:>12}{}",
            workload.name,
            format_millis(workload.slug_median_ns),
            python,
            ratio,
            if workload.held_out {
                "  (hold-out)"
            } else {
                ""
            },
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
