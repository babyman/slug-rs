# Agent Evaluation

This harness measures whether three agent-facing features help an agent
write and review Slug: the [value JSON form](../../docs/reference/value-json.md),
the [capability report](../../docs/reference/capability-report.md), and
[diagnostics](../../docs/reference/diagnostics.md) with stable codes, ranges,
expected and actual values, and fixes. It runs Claude Code headless on small
tasks, once per feature arm and several times each, and records what happened.

## Tasks

- `tasks/` holds 22 coding tasks. Each has a `task.md` prompt, a
  `reference.slug` solution, and `cases.json`. The agent writes
  `solution.slug`; the harness then runs it on every case and compares its
  output with the reference's. The first case uses each `cfg` default from the
  prompt; the others are hidden inputs supplied through a `slug.toml`, so a
  hard-coded answer fails.
- `review/` holds 6 review tasks. Each has `before/` and `after/` versions of
  a program and an `answer.json`. The agent decides whether the change gives
  the program new kinds of access, such as the filesystem or configuration,
  including access gained indirectly or by dynamic import.

- `projects/` holds larger exercises that produce something useful. `json/`
  asks for a JSON library, `json.slug`, specified in its `task.md`, which the
  agent receives as `spec.md` in its workspace. Its hidden
  suite, `cases.json`, has 91 cases covering parsing, parse errors and their
  offsets, canonical serialization, serialization errors, and round trips.
  Each case runs as its own program beside the agent's module, so a crash or
  hang fails only that case, and a run scores the fraction of cases passed.
  `reference/json.slug` passes them all. Project runs default to a $5 budget
  and a 45-minute timeout and use the coding arms.

The small coding tasks are a quick smoke check after language changes; the
projects are the main experiment.

Write new tasks without looking at how the features behave, keep some aside as
hold-outs, and replace a hold-out once it has driven a change.

## Arms

| Arm        | Diagnostics | Fixes   | `slug report` | Tasks          |
|------------|-------------|---------|---------------|----------------|
| `baseline` | text        | -       | blocked       | coding, review |
| `json`     | JSON        | removed | blocked       | coding         |
| `fixes`    | JSON        | kept    | blocked       | coding         |
| `report`   | text        | -       | available     | review         |

The agent's `slug` is a wrapper (`harness/slug_wrapper.py`) that applies the
arm: it forces or strips `--diagnostic-format=json`, empties `fixes`, and
rejects `slug report`. Each arm's prompt and copied documentation mention only
the features it enables. After a failed run the wrapper reruns the command in
JSON mode, only to log its diagnostic codes, so every arm records which
failure classes the agent hit.

## Running

```sh
# Check that every reference solution, including project references, passes its cases.
python3 eval/agent/harness/run.py check-references

# Show the runs a selection would start, and their worst-case cost.
python3 eval/agent/harness/run.py plan --arms baseline,fixes --tasks 01,04 --repeats 3

# Start them. Nothing runs without --yes.
python3 eval/agent/harness/run.py run --arms baseline,fixes --tasks 01,04 --repeats 3 --yes

# Summarize a finished or partial run.
python3 eval/agent/harness/run.py summarize eval/agent/results/<run>
```

Each run is `claude -p` in a fresh workspace with `--permission-mode dontAsk`,
the tools `Bash, Read, Write, Edit, Glob, Grep`, and `--max-budget-usd`
(default 1). By default it uses your normal login with
`--setting-sources project` plus settings that disable hooks and auto-memory
and exclude every CLAUDE.md, so your own settings, CLAUDE.md, memory, and
hooks do not reach the agent. `--setting-sources` alone still loads
`~/.claude/CLAUDE.md`. With `ANTHROPIC_API_KEY` set,
`--bare` isolates it further. `--model` selects the model and `--jobs` runs
several at once. `--tasks` takes name prefixes and matches coding and review
tasks alike, so `--tasks 01` selects both `01-fizzbuzz` and `01-add-file-log`.

The full default plan is 234 runs. Start with a few tasks and repeats.

## Results

Results go to `eval/agent/results/<run>/`, which git ignores:

- `runs.jsonl`: one record per run with the task, arm, repeat, pass or fail,
  cases passed, Claude's turns, tokens, cost, and duration, the number of
  `slug` invocations and failures, and every diagnostic code logged.
- `<task>/<arm>/<repeat>/`: the agent's workspace, Claude's full `stream-json` session output, and
  the wrapper's invocation log, for reading what happened.

A run that ends in an error after one or two turns, typically because access
ran out, is marked `infra_error` and left out of summaries.

`summarize` prints, per kind and arm, the pass rate, the mean project score, and mean turns, `slug`
runs, failed runs, output tokens, cost, and wall time, followed by every
diagnostic code seen and the tasks that failed. Agent output is noisy: compare
arms over several repeats, not single runs.
