#!/usr/bin/env python3
"""Runs a coding agent, Codex or Claude Code, headless on Slug tasks and records
one JSON line per run. The `python` arm implements a project in Python
instead, as a control, for projects that ship checks.json and python/task.md.

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
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
EVAL = HERE.parent
REPO = EVAL.parent.parent
sys.path.insert(0, str(HERE))

from cases import ENTRY, load_cases, run_case  # noqa: E402
import projects  # noqa: E402

TASKS = EVAL / "tasks"
REVIEWS = EVAL / "review"
PROJECTS = EVAL / "projects"
# Per-kind run limits; --max-budget-usd and --timeout-minutes override them.
KIND_LIMITS = {
    "coding": {"budget": 1.0, "minutes": 15},
    "review": {"budget": 1.0, "minutes": 15},
    "project": {"budget": 5.0, "minutes": 45},
}
RESULTS = EVAL / "results"
REVIEW_KINDS = ["filesystem", "stdin", "database", "configuration", "native", "dynamic_import"]

# Each Slug arm is one combination of the three features under test. The
# `python` arm writes the project in Python, with none of them.
ARMS = {
    "baseline": {"diagnostics": "text", "fixes": "off", "report": "off"},
    "json": {"diagnostics": "json", "fixes": "off", "report": "off"},
    "fixes": {"diagnostics": "json", "fixes": "on", "report": "off"},
    "report": {"diagnostics": "text", "fixes": "off", "report": "on"},
    "python": {"diagnostics": "text", "fixes": "off", "report": "off", "language": "python"},
}
# The interpreter Python-arm agents and their grading use: the one running
# the harness.
PYTHON = sys.executable


def language(run):
    return ARMS[run["arm"]].get("language", "slug")

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
# Library modules a project asks agents to write, with their source files,
# withheld from every run's documentation and install so no agent finds one
# ready-made: `slug.fmt.json` is the JSON project's chosen solution.
WITHHELD_MODULES = {"slug.fmt.json": "json.slug"}

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
`slug <file>`. Single-quoted Slug strings are raw: '\\n' is a backslash and an
`n`, not a newline. Use double quotes, with `\\"` for a quote, when a string
needs escapes.

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

PROJECT_PROMPT = """\
You are writing a library in Slug, a small programming language. Its
documentation is in ./slug-docs: start with slug-docs/language-specification.md.
Library module sources are in ./slug-docs/library. Run a program with
`slug <file>`. Single-quoted Slug strings are raw: '\\n' is a backslash and an
`n`, not a newline. Use double quotes, with `\\"` for a quote, when a string
needs escapes.

The library's specification is in ./spec.md. Implement it exactly.
{arm_notes}
Stop when the library is complete and you have tested it.
"""

PYTHON_PROJECT_PROMPT = """\
You are writing a library in Python. Run Python with `python3`.

The library's specification is in ./spec.md. Implement it exactly.

Stop when the library is complete and you have tested it.
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
    for kind, root in (("coding", TASKS), ("review", REVIEWS), ("project", PROJECTS)):
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
            if kind in ("coding", "project") and arm == "report":
                continue
            if kind == "review" and arm in ("json", "fixes"):
                continue
            if arm == "python" and not (kind == "project" and projects.supports_python(directory)):
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


def prepare_workspace(run, run_temp):
    # Outside the repository, so the agent cannot find references or hidden
    # cases by searching parent directories. It is copied into the results
    # after the run.
    workspace = run_temp / "workspace"
    if language(run) == "python":
        workspace.mkdir()
        shutil.copyfile(PROJECTS / run["task"] / "python" / "task.md", workspace / "spec.md")
        return workspace
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
        shutil.copytree(
            REPO / source,
            docs / target,
            ignore=shutil.ignore_patterns(".DS_Store", *WITHHELD_MODULES.values()),
        )
    if run["kind"] == "project":
        shutil.copyfile(PROJECTS / run["task"] / "task.md", workspace / "spec.md")
    if run["kind"] == "review":
        for side in ("before", "after"):
            shutil.copytree(REVIEWS / run["task"] / side, workspace / side)
    return workspace


def budget(run, arguments):
    return arguments.max_budget_usd or KIND_LIMITS[run["kind"]]["budget"]


def minutes(run, arguments):
    return arguments.timeout_minutes or KIND_LIMITS[run["kind"]]["minutes"]


def prompt(run):
    notes = ARM_NOTES.get(run["arm"], "")
    if language(run) == "python":
        return PYTHON_PROJECT_PROMPT
    if run["kind"] == "project":
        return PROJECT_PROMPT.format(arm_notes=notes)
    if run["kind"] == "coding":
        task = (TASKS / run["task"] / "task.md").read_text().strip()
        return CODING_PROMPT.format(task=task, arm_notes=notes)
    return REVIEW_PROMPT.format(arm_notes=notes)


AGENTS = ("codex", "claude")


def agent_command(run, arguments, workspace, log_directory, codex_home):
    if arguments.agent == "codex":
        return codex_command(run, arguments, workspace, log_directory), {"CODEX_HOME": str(codex_home)}
    return claude_command(run, arguments), {}


def codex_command(run, arguments, workspace, log_directory):
    """`codex exec` with a sandbox that may write only the workspace, the
    temporary directories, and the wrapper's log, with no network. Codex has no
    spending limit, so the timeout alone bounds a run. Its home is a fresh
    directory (see `isolated_codex_home`), so the user's config, skills,
    plugins, memories, rules, and hooks do not reach the agent."""
    command = [
        "codex",
        "exec",
        "--json",
        "--ephemeral",
        "--skip-git-repo-check",
        "--ignore-user-config",
        "--ignore-rules",
        "--sandbox",
        "workspace-write",
        "--cd",
        str(workspace),
        "--add-dir",
        str(log_directory),
        "-c",
        'approval_policy="never"',
        "-c",
        'web_search="disabled"',
        "-c",
        'shell_environment_policy.inherit="all"',
    ]
    if arguments.model:
        command += ["--model", arguments.model]
    return command + [prompt(run)]


def isolated_codex_home():
    """A temporary CODEX_HOME holding only a copy of the user's login.

    Returns the directory and the login as copied. A run may refresh the
    login's tokens; `restore_codex_login` copies a refreshed login back so a
    rotated refresh token is not lost."""
    real = Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex") / "auth.json"
    home = Path(tempfile.mkdtemp(prefix="slug-eval-codex-"))
    original = real.read_bytes() if real.exists() else None
    if original is not None:
        login = home / "auth.json"
        login.write_bytes(original)
        login.chmod(0o600)
    return home, real, original


def restore_codex_login(home, real, original):
    login = home / "auth.json"
    try:
        current = login.read_bytes()
        # Copy back only when the run refreshed the login and nothing else
        # changed the user's own copy meanwhile.
        if original is not None and current != original and real.read_bytes() == original:
            real.write_bytes(current)
    except OSError:
        pass
    shutil.rmtree(home, ignore_errors=True)


def claude_command(run, arguments):
    command = [
        "claude",
        "-p",
        prompt(run),
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--permission-mode",
        "dontAsk",
        "--allowedTools",
        "Bash,Read,Write,Edit,Glob,Grep",
        "--max-budget-usd",
        str(budget(run, arguments)),
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


def isolated_install(slug):
    """Copies everything an agent's `slug` needs into a temporary directory,
    so nothing the agent can see points into the repository, where reference
    solutions and hidden cases live."""
    install = Path(tempfile.mkdtemp(prefix="slug-eval-install-"))
    # Clutch tests are left out: some hold project suites' hidden cases.
    ignore = shutil.ignore_patterns(".DS_Store", "tests", *WITHHELD_MODULES.values())
    for directory in ("lib", "clutch"):
        shutil.copytree(REPO / directory, install / "home" / directory, ignore=ignore)
    for manifest in (install / "home" / "clutch").glob("**/*.toml"):
        lines = manifest.read_text().splitlines(keepends=True)
        kept = [line for line in lines if not any(f'"{name}"' in line for name in WITHHELD_MODULES)]
        manifest.write_text("".join(kept))
    (install / "bin").mkdir()
    shutil.copy2(slug, install / "bin" / "slug-real")
    shutil.copy2(HERE / "slug_wrapper.py", install / "bin" / "slug_wrapper.py")
    shutil.copy2(HERE / "python_wrapper.py", install / "bin" / "python_wrapper.py")
    return install


def wrapper_directory(run_temp, install, run):
    directory = run_temp / "bin"
    directory.mkdir()
    if language(run) == "python":
        script = json.dumps(str(install / "bin" / "python_wrapper.py"))
        names = ("python3", "python")
    else:
        script = json.dumps(str(install / "bin" / "slug_wrapper.py"))
        names = ("slug",)
    for name in names:
        wrapper = directory / name
        wrapper.write_text("#!/bin/sh\nexec " + json.dumps(sys.executable) + " " + script + ' "$@"\n')
        wrapper.chmod(0o755)
    return directory


def execute(run, arguments, slug, install, results_dir, references, codex_home):
    run_dir = results_dir / run["task"] / run["arm"] / str(run["repeat"])
    run_dir.mkdir(parents=True)
    run_temp = Path(tempfile.mkdtemp(prefix="slug-eval-run-"))
    workspace = prepare_workspace(run, run_temp)
    # The log has a directory of its own, the one place outside the workspace
    # that a sandboxed agent's `slug` may write.
    log_directory = run_temp / "log"
    log_directory.mkdir()
    log = log_directory / "slug-invocations.jsonl"
    log.touch()
    arm = ARMS[run["arm"]]
    environment = {
        **os.environ,
        "PATH": str(wrapper_directory(run_temp, install, run)) + os.pathsep + os.environ.get("PATH", ""),
        "SLUG_EVAL_PYTHON": PYTHON,
        "SLUG_HOME": str(install / "home"),
        "SLUG_EVAL_REAL": str(install / "bin" / "slug-real"),
        "SLUG_EVAL_LOG": str(log),
        "SLUG_EVAL_DIAGNOSTICS": arm["diagnostics"],
        "SLUG_EVAL_FIXES": arm["fixes"],
        "SLUG_EVAL_REPORT": arm["report"],
    }
    command, agent_environment = agent_command(run, arguments, workspace, log_directory, codex_home)
    environment.update(agent_environment)
    started = time.monotonic()
    try:
        completed = subprocess.run(
            command,
            cwd=workspace,
            env=environment,
            capture_output=True,
            text=True,
            timeout=minutes(run, arguments) * 60,
        )
        timed_out = False
        stdout, stderr = completed.stdout, completed.stderr
    except subprocess.TimeoutExpired as error:
        timed_out = True
        stdout = error.stdout.decode() if isinstance(error.stdout, bytes) else (error.stdout or "")
        stderr = error.stderr.decode() if isinstance(error.stderr, bytes) else (error.stderr or "")
    (run_dir / f"{arguments.agent}-stream.jsonl").write_text(stdout)
    (run_dir / f"{arguments.agent}-stderr.txt").write_text(stderr)
    summary = codex_summary(stdout) if arguments.agent == "codex" else stream_summary(stdout)

    invocations = [json.loads(line) for line in log.read_text().splitlines() if line.strip()]
    record = {
        **run,
        # A run that ends in an error after a turn or two never worked on the
        # task, typically because access ran out; summaries leave it out.
        "infra_error": not summary or bool(summary.get("is_error")) and (summary.get("num_turns") or 0) <= 2,
        "wall_seconds": round(time.monotonic() - started, 1),
        "timed_out": timed_out,
        "agent": arguments.agent,
        "model": arguments.model,
        "session": summary,
        "slug_runs": len(invocations),
        "slug_failures": sum(1 for item in invocations if item.get("exit") not in (0, None)),
        "codes": [code for item in invocations for code in item.get("codes", [])],
        "report_runs": sum(1 for item in invocations if item.get("command") == "report"),
        "language": language(run),
    }
    if language(run) == "python":
        # Codex runs commands in a login shell, whose profile puts /usr/bin
        # ahead of the wrapper, so its Python runs come from its own command
        # log instead. `slug_runs` counts commands that run Python here,
        # including scripts that edit files; `program_runs` breaks them down
        # by how the program was given.
        if arguments.agent == "codex":
            invocations = codex_python_commands(stdout)
            record["slug_runs"] = len(invocations)
            record["slug_failures"] = sum(1 for item in invocations if item.get("exit") not in (0, None))
        record["python_version"] = platform.python_version()
        record["program_runs"] = dict(collections.Counter(item.get("program") for item in invocations))
    record.update(check(run, workspace, slug, references))
    shutil.copytree(workspace, run_dir / "workspace", ignore=shutil.ignore_patterns("slug-docs"))
    shutil.copy2(log, run_dir / "slug-invocations.jsonl")
    shutil.rmtree(run_temp, ignore_errors=True)
    (run_dir / "record.json").write_text(json.dumps(record, indent=2))
    return record


def stream_summary(stdout):
    """Totals for a whole session from Claude Code's stream-json output.

    The final `result` event alone describes only the last exchange, which a
    late background-task notification can make a trivial one, so turns and
    tool calls are counted from every assistant message, and tokens and cost
    come from the cumulative per-model usage.
    """
    events = []
    for line in stdout.splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            continue
    assistants = [event for event in events if event.get("type") == "assistant"]
    results = [event for event in events if event.get("type") == "result"]
    if not results:
        return {}
    last = results[-1]
    models = last.get("modelUsage") or {}
    total = lambda key: sum(model.get(key) or 0 for model in models.values())
    tools = collections.Counter(
        block.get("name")
        for event in assistants
        for block in (event.get("message") or {}).get("content") or []
        if isinstance(block, dict) and block.get("type") == "tool_use"
    )
    return {
        "subtype": last.get("subtype"),
        "is_error": any(result.get("is_error") for result in results),
        "num_turns": len(assistants),
        "tool_calls": dict(tools),
        "results": len(results),
        "total_cost_usd": last.get("total_cost_usd"),
        "usage": {
            "output_tokens": total("outputTokens"),
            "thinking_tokens": total("thinkingTokens"),
            "input_tokens": total("inputTokens"),
            "cache_read_input_tokens": total("cacheReadInputTokens"),
            "cache_creation_input_tokens": total("cacheCreationInputTokens"),
        },
    }


def codex_summary(stdout):
    """Totals for a session from `codex exec --json` output.

    Each completed item is one agent step: a message, a command, a file
    change, or another tool call. `num_turns` counts the steps that are not
    reasoning, which is close to, but not the same measure as, Claude Code's
    assistant messages. Codex reports tokens but no cost.
    """
    events = []
    for line in stdout.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict):
            events.append(event)
    if not events:
        return {}
    items = [
        event.get("item") or {}
        for event in events
        if event.get("type") == "item.completed"
    ]
    steps = collections.Counter(item.get("type") for item in items)
    usage = collections.Counter()
    for event in events:
        if event.get("type") == "turn.completed":
            usage.update({key: value for key, value in (event.get("usage") or {}).items() if isinstance(value, int)})
    failures = [event for event in events if event.get("type") in ("turn.failed", "error")]
    return {
        "subtype": "error" if failures else "success",
        "is_error": bool(failures),
        "errors": [
            (event.get("error") or {}).get("message") if isinstance(event.get("error"), dict) else event.get("message")
            for event in failures
        ],
        "num_turns": sum(count for kind, count in steps.items() if kind != "reasoning"),
        "tool_calls": {kind: count for kind, count in steps.items() if kind not in ("agent_message", "reasoning")},
        "total_cost_usd": None,
        "usage": {
            "output_tokens": usage.get("output_tokens", 0),
            "thinking_tokens": usage.get("reasoning_output_tokens", 0),
            "input_tokens": usage.get("input_tokens", 0),
            "cache_read_input_tokens": usage.get("cached_input_tokens", 0),
        },
    }


PYTHON_COMMAND = re.compile(r"(?:^|[\s;&|(\"'])(?:[^\s\"']*/)?python3?(?=\s|$)(.*)")


def codex_python_commands(stdout):
    """The commands in a Codex session that run Python, with their exit codes
    and how each gave its program: a script, `-c`, `-m`, or standard input,
    as heredoc edit scripts do."""
    found = []
    for line in stdout.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        item = event.get("item") or {} if isinstance(event, dict) else {}
        if event.get("type") != "item.completed" or item.get("type") != "command_execution":
            continue
        command = item.get("command") or ""
        for text in command.splitlines():
            match = PYTHON_COMMAND.search(text)
            if not match:
                continue
            rest = match.group(1).split()
            if not rest or rest[0] in ("-", "<<", "<") or rest[0].startswith("<<"):
                program = "stdin"
            elif rest[0] == "-c":
                program = "inline"
            elif rest[0] == "-m":
                program = "module"
            else:
                program = "script"
            found.append({"command": text.strip()[:200], "program": program, "exit": item.get("exit_code")})
            break
    return found


def check(run, workspace, slug, references):
    if language(run) == "python":
        module = run["task"]
        source = workspace / f"{module}.py"
        total = len(projects.load_checks(PROJECTS / run["task"]))
        if not source.exists():
            return {"passed": False, "score": 0.0, "cases_passed": 0, "cases": total, "detail": f"no {module}.py"}
        outcomes = projects.grade_python(PYTHON, PROJECTS / run["task"], source, module)
        passed = sum(outcome["passed"] for outcome in outcomes)
        return {
            "passed": passed == total,
            "score": round(passed / total, 4),
            "cases_passed": passed,
            "cases": total,
            "failed_cases": [outcome["id"] for outcome in outcomes if not outcome["passed"]],
        }
    if run["kind"] == "project":
        module = run["task"]
        source = workspace / f"{module}.slug"
        total = len(projects.load_cases(PROJECTS / run["task"]))
        if not source.exists():
            return {"passed": False, "score": 0.0, "cases_passed": 0, "cases": total, "detail": f"no {module}.slug"}
        outcomes = projects.grade(str(slug), REPO, PROJECTS / run["task"], source, module)
        passed = sum(outcome["passed"] for outcome in outcomes)
        return {
            "passed": passed == total,
            "score": round(passed / total, 4),
            "cases_passed": passed,
            "cases": total,
            "failed_cases": [outcome["id"] for outcome in outcomes if not outcome["passed"]],
        }
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
    if arguments.agent == "codex":
        longest = sum(minutes(run, arguments) for run in runs)
        print(f"total   {len(runs)} codex runs, at most {longest} minutes in timeouts (codex has no budget limit)")
    else:
        worst = sum(budget(run, arguments) for run in runs)
        print(f"total   {len(runs)} runs, at most ${worst:.2f} in budgets")
    return runs


def command_run(arguments):
    runs = command_plan(arguments)
    if not arguments.yes:
        print("dry run: pass --yes to start these runs")
        return
    slug = build_slug()
    coding = sorted({run["task"] for run in runs if run["kind"] == "coding"})
    references = reference_outputs(slug, coding)
    install = isolated_install(slug)
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    results_dir = RESULTS / (arguments.name or stamp)
    results_dir.mkdir(parents=True)
    (results_dir / "config.json").write_text(
        json.dumps({"arguments": vars(arguments), "arms": ARMS, "git": git_head()}, indent=2, default=str)
    )
    out = results_dir / "runs.jsonl"
    codex = isolated_codex_home() if arguments.agent == "codex" else None
    codex_home = codex[0] if codex else None
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=arguments.jobs) as pool, out.open("a") as sink:
            futures = [
                pool.submit(execute, run, arguments, slug, install, results_dir, references, codex_home)
                for run in runs
            ]
            for future in concurrent.futures.as_completed(futures):
                record = future.result()
                sink.write(json.dumps(record) + "\n")
                sink.flush()
                status = "pass" if record["passed"] else "FAIL"
                session = session_of(record)
                print(f"{status} {record['task']} {record['arm']} #{record['repeat']} "
                      f"turns={session.get('num_turns')} slug_runs={record['slug_runs']} "
                      f"output_tokens={(session.get('usage') or {}).get('output_tokens')} "
                      f"cost={session.get('total_cost_usd')}")
    finally:
        if codex:
            restore_codex_login(*codex)
    shutil.rmtree(install, ignore_errors=True)
    print(f"results: {out}")
    summarize(out)


def git_head():
    try:
        return subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True).stdout.strip()
    except OSError:
        return None


def session_of(record):
    """The agent session summary; records from before Codex support keep it
    under `claude`."""
    return record.get("session") or record.get("claude") or {}


def summarize(path):
    path = Path(path)
    if path.is_dir():
        path = path / "runs.jsonl"
    loaded = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    def infra_error(record):
        session = session_of(record)
        fallback = bool(session.get("is_error")) and (session.get("num_turns") or 0) <= 2
        return record.get("infra_error", fallback)

    records = [record for record in loaded if not infra_error(record)]
    if len(records) < len(loaded):
        print(f"excluding {len(loaded) - len(records)} runs that ended in an access or harness error\n")
    groups = collections.defaultdict(list)
    for record in records:
        groups[(record.get("agent", "claude"), record["kind"], record["arm"])].append(record)

    def mean(values):
        values = [value for value in values if value is not None]
        return statistics.mean(values) if values else None

    def show(value, digits=1):
        return "-" if value is None else f"{value:.{digits}f}"

    print(f"{'agent':6} {'kind':7} {'arm':9} {'runs':>4} {'pass':>5} {'score':>6} {'turns':>6} {'slug':>5} {'fails':>5} {'out_tok':>8} {'cost':>7} {'secs':>6}")
    for (agent, kind, arm), items in sorted(groups.items()):
        sessions = [session_of(item) for item in items]
        usage = [session.get("usage") or {} for session in sessions]
        print(
            f"{agent:6} {kind:7} {arm:9} {len(items):>4} "
            f"{sum(item['passed'] for item in items) / len(items):>5.0%} "
            f"{show(mean([item.get('score') for item in items]), 2):>6} "
            f"{show(mean([session.get('num_turns') for session in sessions])):>6} "
            f"{show(mean([item['slug_runs'] for item in items])):>5} "
            f"{show(mean([item['slug_failures'] for item in items])):>5} "
            f"{show(mean([u.get('output_tokens') for u in usage]), 0):>8} "
            f"{show(mean([session.get('total_cost_usd') for session in sessions]), 3):>7} "
            f"{show(mean([item['wall_seconds'] for item in items])):>6}"
        )
    codes = collections.Counter(code for record in records for code in record["codes"])
    if codes:
        print("\ndiagnostic codes seen:")
        for code, count in codes.most_common():
            print(f"  {count:>4} {code}")
    missed = collections.Counter(case for record in records for case in record.get("failed_cases", []))
    if missed:
        print("\nproject cases failed most often:")
        for case, count in missed.most_common(15):
            print(f"  {count:>4} {case}")
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
    for kind, directory in tasks([]):
        if kind != "project":
            continue
        module = directory.name
        outcomes = projects.grade(str(slug), REPO, directory, directory / "reference" / f"{module}.slug", module)
        # The reference is a naive baseline: cases marked `reference_fails`,
        # such as correctly rounded numbers, are ones it is known to miss.
        # Any other difference means the suite or the reference changed.
        expected = {case["id"] for case in projects.load_cases(directory) if case.get("reference_fails")}
        failed = {outcome["id"] for outcome in outcomes if not outcome["passed"]}
        if failed != expected:
            unexpected = ", ".join(sorted(failed - expected)) or "none"
            fixed = ", ".join(sorted(expected - failed)) or "none"
            raise SystemExit(f"project {module} reference: unexpected failures {unexpected}; now passing {fixed}")
        print(f"project {module} reference passes {len(outcomes) - len(failed)} of {len(outcomes)} cases, failing only its {len(expected)} known cases")
        if projects.supports_python(directory):
            reference = directory / "python" / "reference" / f"{module}.py"
            outcomes = projects.grade_python(PYTHON, directory, reference, module)
            failed = sorted(outcome["id"] for outcome in outcomes if not outcome["passed"])
            if failed:
                raise SystemExit(f"project {module} Python reference fails {', '.join(failed)}")
            print(f"project {module} Python reference passes {len(outcomes)} of {len(outcomes)} checks")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("plan", "run"):
        sub = commands.add_parser(name)
        sub.add_argument("--arms", default="baseline,json,fixes,report")
        sub.add_argument("--tasks", help="comma-separated task name prefixes; default all")
        sub.add_argument("--repeats", type=int, default=3)
        sub.add_argument("--max-budget-usd", type=float, help="per-run budget for claude; default 1, or 5 for projects")
        sub.add_argument("--timeout-minutes", type=int, help="per-run timeout; default 15, or 45 for projects")
        sub.add_argument("--agent", choices=AGENTS, default="codex", help="the agent CLI to run; default codex")
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
