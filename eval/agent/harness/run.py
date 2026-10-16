#!/usr/bin/env python3
"""Runs Claude Code headless on Slug tasks and records one JSON line per run.

    python3 eval/agent/harness/run.py plan --arms baseline,json --repeats 3
    python3 eval/agent/harness/run.py run --arms baseline,json --repeats 3 --yes
    python3 eval/agent/harness/run.py summarize eval/agent/results/<run>
    python3 eval/agent/harness/run.py check-references

See eval/agent/README.md.
"""

import argparse
import collections
import concurrent.futures
import datetime
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
EVAL = HERE.parent
REPO = EVAL.parent.parent
sys.path.insert(0, str(HERE))

from cases import ENTRY, load_cases, run_case  # noqa: E402

TASKS = EVAL / "tasks"
REVIEWS = EVAL / "review"
RESULTS = EVAL / "results"
REVIEW_KINDS = ["filesystem", "stdin", "database", "configuration", "native", "dynamic_import"]

# Each arm is one combination of the three features under test.
ARMS = {
    "baseline": {"diagnostics": "text", "fixes": "off", "report": "off"},
    "json": {"diagnostics": "json", "fixes": "off", "report": "off"},
    "fixes": {"diagnostics": "json", "fixes": "on", "report": "off"},
    "report": {"diagnostics": "text", "fixes": "off", "report": "on"},
}

# Documentation every arm sees, copied into the workspace as slug-docs/.
COMMON_DOCS = [
    "docs/language/language-specification.md",
    "docs/language/runtime-requirements.md",
    "docs/language/configuration.md",
    "docs/language/errors.md",
    "docs/language/strings.md",
    "docs/language/maps.md",
    "docs/language/structs.md",
    "docs/language/match-and-destructuring.md",
    "docs/language/nominal-types.md",
    "docs/language/variadic-functions-and-spread-syntax.md",
    "docs/language/value-pinning-in-match-patterns.md",
    "docs/language/automatic-semicolon-insertion.md",
    "docs/language/standard-input.md",
    "docs/language/filesystem.md",
]
JSON_DOCS = [
    "docs/reference/diagnostics.md",
    "docs/reference/diagnostic-codes.md",
    "docs/reference/value-json.md",
]
REPORT_DOCS = ["docs/reference/capability-report.md"]
LIBRARY_SOURCES = [
    ("lib/slug", "library"),
    ("clutch/slug.core.clutch/modules", "library/core"),
    ("clutch/slug.io.fs.clutch/modules", "library/fs"),
    ("clutch/slug.db.sqlite.clutch/modules", "library/sqlite"),
]

CODING_PROMPT = """\
You are writing a program in Slug, a small programming language. Its
documentation is in ./slug-docs: start with slug-docs/language-specification.md.
Library module sources are in ./slug-docs/library. Run a program with
`slug <file>`.

Write the program to ./solution.slug in the current directory.

{task}

Your program will be tested with other inputs. Tests supply them the way
`cfg` reads configuration: a slug.toml beside the program with a `[solution]`
table, such as

    [solution]
    n = 20

so read every input with `cfg` exactly as described, and do not hard-code
the output. You may create a slug.toml to try other inputs, but delete it
before you finish. Print exactly the required output and nothing else.
{arm_notes}
Stop when solution.slug is correct.
"""

REVIEW_PROMPT = """\
You are reviewing a change to a program written in Slug, a small programming
language. Its documentation is in ./slug-docs: start with
slug-docs/language-specification.md. Library module sources are in
./slug-docs/library. Run a program with `slug <file>`.

The program before the change is in ./before and after the change in ./after.
In both, the entry file is app.slug.

Decide whether the program after the change can access anything outside its
own Slug code that the program before the change could not. Count access
gained through any module the program imports, directly or indirectly. These
are the kinds of access:

- filesystem: files, through slug.io.fs
- stdin: standard input, through slug.io.stdin
- database: SQLite, through slug.db.sqlite
- configuration: configuration values, through cfg
- native: a foreign function declared by the program's own modules
- dynamic_import: an import whose module name is computed at runtime

Write ./answer.json containing an object with `new_access`, true or false,
and `kinds`, the list of kinds of access the after program gained. Access
that was removed is not new access. For example:

    {{"new_access": true, "kinds": ["filesystem"]}}
{arm_notes}
Stop when answer.json is written.
"""

ARM_NOTES = {
    "json": "\n`slug` reports errors as JSON diagnostics, described in\nslug-docs/diagnostics.md.\n",
    "fixes": (
        "\n`slug` reports errors as JSON diagnostics, described in\n"
        "slug-docs/diagnostics.md. A diagnostic's `fixes` are edits that are\n"
        "safe to apply.\n"
    ),
    "report": (
        "\n`slug report <file>` prints a JSON capability report of what a\n"
        "program can touch, described in slug-docs/capability-report.md.\n"
    ),
}


def tasks(selection):
    found = []
    for kind, root in (("coding", TASKS), ("review", REVIEWS)):
        for directory in sorted(path for path in root.iterdir() if path.is_dir()):
            if selection and not any(directory.name.startswith(item) for item in selection):
                continue
            found.append((kind, directory))
    return found


def plan(arguments):
    arms = arguments.arms.split(",")
    for arm in arms:
        if arm not in ARMS:
            raise SystemExit(f"unknown arm `{arm}`; choose from {', '.join(ARMS)}")
    selected = tasks(arguments.tasks.split(",") if arguments.tasks else [])
    runs = []
    for kind, directory in selected:
        for arm in arms:
            # Diagnostics arms only matter for coding; the report arm only
            # for review. Skipping the other pairings keeps runs informative.
            if kind == "coding" and arm == "report":
                continue
            if kind == "review" and arm in ("json", "fixes"):
                continue
            for repeat in range(arguments.repeats):
                runs.append({"kind": kind, "task": directory.name, "arm": arm, "repeat": repeat})
    return runs


def build_slug():
    subprocess.run(
        ["cargo", "build", "--release", "-p", "slug", "--bin", "slug"],
        cwd=REPO,
        check=True,
    )
    return REPO / "target" / "release" / "slug"


def prepare_workspace(run, run_dir):
    workspace = run_dir / "workspace"
    docs = workspace / "slug-docs"
    docs.mkdir(parents=True)
    paths = list(COMMON_DOCS)
    if ARMS[run["arm"]]["diagnostics"] == "json":
        paths += JSON_DOCS
    if ARMS[run["arm"]]["report"] == "on":
        paths += REPORT_DOCS
    for path in paths:
        shutil.copyfile(REPO / path, docs / Path(path).name)
    for source, target in LIBRARY_SOURCES:
        shutil.copytree(REPO / source, docs / target, ignore=shutil.ignore_patterns(".DS_Store"))
    if run["kind"] == "review":
        for side in ("before", "after"):
            shutil.copytree(REVIEWS / run["task"] / side, workspace / side)
    return workspace


def prompt(run):
    notes = ARM_NOTES.get(run["arm"], "")
    if run["kind"] == "coding":
        task = (TASKS / run["task"] / "task.md").read_text().strip()
        return CODING_PROMPT.format(task=task, arm_notes=notes)
    return REVIEW_PROMPT.format(arm_notes=notes)


def claude_command(run, arguments):
    command = [
        "claude",
        "-p",
        prompt(run),
        "--output-format",
        "json",
        "--no-session-persistence",
        "--permission-mode",
        "dontAsk",
        "--allowedTools",
        "Bash,Read,Write,Edit,Glob,Grep",
        "--max-budget-usd",
        str(arguments.max_budget_usd),
    ]
    if arguments.model:
        command += ["--model", arguments.model]
    if arguments.bare:
        command += ["--bare"]
    else:
        # Keep the user's own settings, CLAUDE.md, memory, and hooks out of
        # the run; a hook that rewrites command output would change what the
        # agent sees. --setting-sources alone still loads ~/.claude/CLAUDE.md.
        isolation = {
            "disableAllHooks": True,
            "autoMemoryEnabled": False,
            "claudeMdExcludes": [os.path.expanduser("~/.claude/CLAUDE.md"), "**/CLAUDE.md"],
        }
        command += ["--setting-sources", "project", "--settings", json.dumps(isolation)]
    return command


def wrapper_directory(run_dir):
    directory = run_dir / "bin"
    directory.mkdir()
    wrapper = directory / "slug"
    wrapper.write_text(
        "#!/bin/sh\nexec " + json.dumps(sys.executable) + " " + json.dumps(str(HERE / "slug_wrapper.py")) + ' "$@"\n'
    )
    wrapper.chmod(0o755)
    return directory


def execute(run, arguments, slug, results_dir, references):
    run_dir = results_dir / run["task"] / run["arm"] / str(run["repeat"])
    run_dir.mkdir(parents=True)
    workspace = prepare_workspace(run, run_dir)
    log = run_dir / "slug-invocations.jsonl"
    log.touch()
    arm = ARMS[run["arm"]]
    environment = {
        **os.environ,
        "PATH": str(wrapper_directory(run_dir)) + os.pathsep + os.environ.get("PATH", ""),
        "SLUG_HOME": str(REPO),
        "SLUG_EVAL_REAL": str(slug),
        "SLUG_EVAL_LOG": str(log),
        "SLUG_EVAL_DIAGNOSTICS": arm["diagnostics"],
        "SLUG_EVAL_FIXES": arm["fixes"],
        "SLUG_EVAL_REPORT": arm["report"],
    }
    started = time.monotonic()
    try:
        completed = subprocess.run(
            claude_command(run, arguments),
            cwd=workspace,
            env=environment,
            capture_output=True,
            text=True,
            timeout=arguments.timeout_minutes * 60,
        )
        timed_out = False
        stdout, stderr = completed.stdout, completed.stderr
    except subprocess.TimeoutExpired as error:
        timed_out = True
        stdout = error.stdout.decode() if isinstance(error.stdout, bytes) else (error.stdout or "")
        stderr = error.stderr.decode() if isinstance(error.stderr, bytes) else (error.stderr or "")
    (run_dir / "claude-stdout.json").write_text(stdout)
    (run_dir / "claude-stderr.txt").write_text(stderr)
    try:
        claude = json.loads(stdout)
    except ValueError:
        claude = {}

    invocations = [json.loads(line) for line in log.read_text().splitlines() if line.strip()]
    record = {
        **run,
        "wall_seconds": round(time.monotonic() - started, 1),
        "timed_out": timed_out,
        "claude": {
            key: claude.get(key)
            for key in ("subtype", "is_error", "num_turns", "duration_ms", "duration_api_ms", "total_cost_usd", "usage")
        },
        "slug_runs": len(invocations),
        "slug_failures": sum(1 for item in invocations if item.get("exit") not in (0, None)),
        "codes": [code for item in invocations for code in item.get("codes", [])],
        "report_runs": sum(1 for item in invocations if item.get("command") == "report"),
    }
    record.update(check(run, workspace, slug, references))
    (run_dir / "record.json").write_text(json.dumps(record, indent=2))
    return record


def check(run, workspace, slug, references):
    if run["kind"] == "review":
        expected = json.loads((REVIEWS / run["task"] / "answer.json").read_text())
        try:
            answer = json.loads((workspace / "answer.json").read_text())
        except (OSError, ValueError):
            return {"passed": False, "detail": "missing or invalid answer.json"}
        passed = (
            isinstance(answer, dict)
            and answer.get("new_access") == expected["new_access"]
            and isinstance(answer.get("kinds"), list)
            and set(answer["kinds"]) == set(expected["kinds"])
        )
        return {"passed": passed, "answer": answer}
    solution = workspace / ENTRY
    if not solution.exists():
        return {"passed": False, "cases_passed": 0, "cases": len(references[run["task"]]), "detail": "no solution.slug"}
    outcomes = []
    for case, expected in zip(load_cases(TASKS / run["task"]), references[run["task"]]):
        code, stdout, _ = run_case(str(slug), REPO, solution, case)
        outcomes.append(code == 0 and stdout == expected)
    return {"passed": all(outcomes), "cases_passed": sum(outcomes), "cases": len(outcomes)}


def reference_outputs(slug, names):
    outputs = {}
    for name in names:
        directory = TASKS / name
        outputs[name] = []
        for case in load_cases(directory):
            code, stdout, stderr = run_case(str(slug), REPO, directory / "reference.slug", case)
            if code != 0:
                raise SystemExit(f"reference for {name} failed on {case}: {stderr}")
            outputs[name].append(stdout)
    return outputs


def command_plan(arguments):
    runs = plan(arguments)
    counts = collections.Counter((run["kind"], run["arm"]) for run in runs)
    for (kind, arm), count in sorted(counts.items()):
        print(f"{kind:7} {arm:9} {count} runs")
    print(f"total   {len(runs)} runs, at most ${len(runs) * arguments.max_budget_usd:.2f} at --max-budget-usd {arguments.max_budget_usd}")
    return runs


def command_run(arguments):
    runs = command_plan(arguments)
    if not arguments.yes:
        print("dry run: pass --yes to start these runs")
        return
    slug = build_slug()
    coding = sorted({run["task"] for run in runs if run["kind"] == "coding"})
    references = reference_outputs(slug, coding)
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    results_dir = RESULTS / (arguments.name or stamp)
    results_dir.mkdir(parents=True)
    (results_dir / "config.json").write_text(
        json.dumps({"arguments": vars(arguments), "arms": ARMS, "git": git_head()}, indent=2, default=str)
    )
    out = results_dir / "runs.jsonl"
    with concurrent.futures.ThreadPoolExecutor(max_workers=arguments.jobs) as pool, out.open("a") as sink:
        futures = [pool.submit(execute, run, arguments, slug, results_dir, references) for run in runs]
        for future in concurrent.futures.as_completed(futures):
            record = future.result()
            sink.write(json.dumps(record) + "\n")
            sink.flush()
            status = "pass" if record["passed"] else "FAIL"
            print(f"{status} {record['task']} {record['arm']} #{record['repeat']} "
                  f"turns={record['claude'].get('num_turns')} slug_runs={record['slug_runs']} "
                  f"cost={record['claude'].get('total_cost_usd')}")
    print(f"results: {out}")
    summarize(out)


def git_head():
    try:
        return subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True).stdout.strip()
    except OSError:
        return None


def summarize(path):
    path = Path(path)
    if path.is_dir():
        path = path / "runs.jsonl"
    records = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    groups = collections.defaultdict(list)
    for record in records:
        groups[(record["kind"], record["arm"])].append(record)

    def mean(values):
        values = [value for value in values if value is not None]
        return statistics.mean(values) if values else None

    def show(value, digits=1):
        return "-" if value is None else f"{value:.{digits}f}"

    print(f"{'kind':7} {'arm':9} {'runs':>4} {'pass':>5} {'turns':>6} {'slug':>5} {'fails':>5} {'out_tok':>8} {'cost':>7} {'secs':>6}")
    for (kind, arm), items in sorted(groups.items()):
        usage = [item["claude"].get("usage") or {} for item in items]
        print(
            f"{kind:7} {arm:9} {len(items):>4} "
            f"{sum(item['passed'] for item in items) / len(items):>5.0%} "
            f"{show(mean([item['claude'].get('num_turns') for item in items])):>6} "
            f"{show(mean([item['slug_runs'] for item in items])):>5} "
            f"{show(mean([item['slug_failures'] for item in items])):>5} "
            f"{show(mean([u.get('output_tokens') for u in usage]), 0):>8} "
            f"{show(mean([item['claude'].get('total_cost_usd') for item in items]), 3):>7} "
            f"{show(mean([item['wall_seconds'] for item in items])):>6}"
        )
    codes = collections.Counter(code for record in records for code in record["codes"])
    if codes:
        print("\ndiagnostic codes seen:")
        for code, count in codes.most_common():
            print(f"  {count:>4} {code}")
    failing = collections.Counter(record["task"] for record in records if not record["passed"])
    if failing:
        print("\nfailing tasks:")
        for task, count in failing.most_common():
            print(f"  {count:>4} {task}")


def command_check_references(arguments):
    slug = build_slug()
    names = [directory.name for kind, directory in tasks([]) if kind == "coding"]
    outputs = reference_outputs(slug, names)
    print(f"{len(names)} coding references pass {sum(len(cases) for cases in outputs.values())} cases")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("plan", "run"):
        sub = commands.add_parser(name)
        sub.add_argument("--arms", default="baseline,json,fixes,report")
        sub.add_argument("--tasks", help="comma-separated task name prefixes; default all")
        sub.add_argument("--repeats", type=int, default=3)
        sub.add_argument("--max-budget-usd", type=float, default=1.0)
        sub.add_argument("--timeout-minutes", type=int, default=15)
        sub.add_argument("--model")
        sub.add_argument("--jobs", type=int, default=1)
        sub.add_argument("--name", help="results directory name; default a timestamp")
        sub.add_argument("--bare", action="store_true", help="run claude --bare (needs ANTHROPIC_API_KEY)")
        sub.add_argument("--yes", action="store_true", help="actually start the runs")
    summary = commands.add_parser("summarize")
    summary.add_argument("path")
    commands.add_parser("check-references")
    arguments = parser.parse_args()
    if arguments.command == "plan":
        command_plan(arguments)
    elif arguments.command == "run":
        command_run(arguments)
    elif arguments.command == "summarize":
        summarize(arguments.path)
    else:
        command_check_references(arguments)


if __name__ == "__main__":
    main()
