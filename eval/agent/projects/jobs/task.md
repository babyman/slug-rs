# Job runner

Write a Slug module, `./jobs.slug`, that runs a set of jobs with dependencies
between them, concurrently, with retries, timeouts, and an optional log file.
It will be graded by a hidden suite of programs that import it with
`val jobs = import("jobs")`.

Export exactly these five names.

## `Status`

An enum with the cases `Ok`, `Failed`, `Skipped`, and `TimedOut`, so callers
can write `jobs.Status.Ok`.

## `Job`

A struct schema with these fields, so callers can write
`jobs.Job { name: "build", deps: ["fetch"], action: fn(inputs) { ... } }`:

| Field | Default | Meaning |
|---|---|---|
| `name` | required | The job's name, a string, unique within one job list. |
| `action` | required | A function of one argument, `inputs`, that does the job's work and returns its result, which may be any value, including `nil`. |
| `deps` | `[]` | The names of the jobs this job depends on. A name may repeat; repeats have no further effect. |
| `retries` | `0` | How many more attempts to make after a failed or timed-out one. |
| `timeout` | `0` | Milliseconds an attempt may take before it counts as timed out; `0` means no limit. |

## `Outcome`

A struct schema describing how a job ended, with these fields:

| Field | Meaning |
|---|---|
| `status` | A `Status` case. |
| `value` | The action's result when `status` is `Ok`, otherwise `nil`. |
| `attempts` | The number of times the action was called. |
| `error` | `nil` when `status` is `Ok` or `Skipped`; the `msg` of the last error the action threw when `Failed`; `"timeout"` when `TimedOut`. |

## `plan(jobs:list)`

Returns the order in which `jobs`, a list of `Job` values, can run, as a list
of waves. Each wave is a list of job names. The first wave holds every job with
no dependencies; each later wave holds every job not in an earlier wave whose
dependencies are all in earlier waves. Names within a wave are sorted in
ascending order by Slug's string ordering (`<`). `plan([])` is `[]`. `plan`
never calls an action.

An invalid job list throws an `Error` (from `slug.builtin`) with
`type: "jobs.invalid"`, any `msg`, and a `data` map. The checks are made in this
order, and the first that fails is reported:

1. **Duplicate names.** `data` is `{reason: "duplicate", name: n}`, where `n` is
   the first name in list order that has already appeared earlier in the list.
2. **Missing dependencies.** `data` is
   `{reason: "missing", name: n, dependency: d}`, for the first job in list
   order with a dependency that names no job, and `d` its first such
   dependency, in `deps` order.
3. **Cycles.** `data` is `{reason: "cycle", names: ns}`, where `ns` is the
   sorted list of the names of every job that can never be placed in a wave:
   the jobs on a dependency cycle and the jobs that depend on one, directly or
   indirectly.

## `run(jobs:list, concurrency = 4, log = nil)`

Checks `jobs` as `plan` does, throwing the same errors before calling any
action, and then runs them. `concurrency` must be a positive integer; any other
value throws `jobs.invalid` with `data` `{reason: "concurrency"}`, also before
calling any action.

- A job is ready once every one of its dependencies has finished with status
  `Ok`. Its action is then called with `inputs`, a map from the name of each of
  its own dependencies, and no other job, to that dependency's result. A job
  with no dependencies gets `{}`.
- At most `concurrency` actions run at the same time. When at least
  `concurrency` jobs are ready, `concurrency` actions run at once, so
  independent jobs overlap: actions that wait, for example on
  `select { after 50 }`, wait together rather than one after another.
- An attempt fails when the action throws. With `timeout` greater than `0`, an
  attempt also fails when it has not returned within `timeout` milliseconds;
  the runner stops waiting for it, ignores whatever it later returns or
  throws, and treats it as timed out. A failed or timed-out attempt is retried,
  with the same `inputs`, until `retries` further attempts have been made. The
  job's status is `Ok` if an attempt returns, and otherwise `TimedOut` if the
  last attempt timed out, or `Failed` if it threw.
- A job with a dependency whose status is not `Ok` is `Skipped`: its action is
  never called, and so is skipped in turn every job that depends on it.
  Failures never stop independent jobs: every job that can run does.
- `run` returns a map from every job's name to its `Outcome`, once every job
  has finished. It may also wait for attempts that timed out to return.

When `log` is a string, `run` writes a file at that path, replacing any file
already there, with one line per job as it finishes:
`name status attempts`, where `status` is `ok`, `failed`, `skipped`, or
`timed_out`, and each line ends with a line feed; for example `build ok 1`. A
job's line comes after the lines of all its dependencies. An empty job list
writes an empty file.

## Notes

- Concurrency is covered in the language specification's "Concurrency,
  channels, and `select`" section; `slug.channel` and `slug.io.fs` are in
  slug-docs/library, and `slug.std` provides `map`, `filter`, and `reduce`.
- Write your own test programs beside `jobs.slug` to check your work. The
  graded suite covers every rule above, including timing, concurrency limits,
  error containment, and lists of a thousand jobs that must be handled in a few
  seconds.
