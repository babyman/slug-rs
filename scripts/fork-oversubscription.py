#!/usr/bin/env python3
"""What running more CPU-bound forks than cores costs: wall time, CPU time,
and involuntary context switches for a fixed total of work, split across
8 to 128 forks, unpaced and paced with `nursery limit`. Configurations
interleave round by round; medians are reported.

    python3 -I scripts/fork-oversubscription.py target/release/slug [--rounds N]

The parked M:N experiment in `docs/planning/fork-tasks.md` records what it
showed.
"""
import argparse, os, resource, statistics, subprocess, tempfile, time

parser = argparse.ArgumentParser()
parser.add_argument("slug")
parser.add_argument("--rounds", type=int, default=5)
options = parser.parse_args()
SLUG = os.path.abspath(options.slug)
OUT = tempfile.mkdtemp(prefix="slug-oversubscription-")
ROUNDS = options.rounds
TOTAL = 128_000_000

TEMPLATE = """
val spin = fn(i:num, acc:num):num {{ if (i == 0) {{ acc }} else {{ recur(i - 1, (acc * 31 + i) % 1000003) }} }}
val start = fn(i:num, handles:list):list {{
    if (i == {forks}) {{ handles }} else {{ recur(i + 1, handles :+ fork {{ spin({share}, 0) }}) }}
}}
val collect = fn(handles:list, i:num, acc:num):num {{
    if (i == len(handles)) {{ acc }} else {{ recur(handles, i + 1, acc + select {{ await handles[i] }}) }}
}}
val main = fn() {{
    val total = nursery{limit} {{ collect(start(0, []), 0, 0) }}
    println(total)
}}
"""

CONFIGS = [(8, None), (16, None), (32, None), (64, None), (128, None),
           (32, 8), (64, 8), (128, 8), (32, 4)]

def program(forks, limit):
    path = os.path.join(OUT, f"f{forks}-l{limit or 0}.slug")
    with open(path, "w") as f:
        f.write(TEMPLATE.format(forks=forks, share=TOTAL // forks,
                                limit=f" limit {limit}" if limit else ""))
    return path

def run(path):
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    started = time.perf_counter()
    subprocess.run([SLUG, path], check=True, capture_output=True)
    wall = time.perf_counter() - started
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
    switches = after.ru_nivcsw - before.ru_nivcsw
    return wall, cpu, switches

paths = {config: program(*config) for config in CONFIGS}
samples = {config: [] for config in CONFIGS}
for _ in range(ROUNDS):
    for config in CONFIGS:
        samples[config].append(run(paths[config]))

base = statistics.median(w for w, _, _ in samples[(8, None)])
print(f"{'forks':>6} {'limit':>6} {'wall s':>8} {'vs 8':>6} {'spread':>7} {'cpu s':>7} {'inv. switches':>14}")
for config in CONFIGS:
    walls = [w for w, _, _ in samples[config]]
    wall = statistics.median(walls)
    spread = (max(walls) - min(walls)) / wall
    cpu = statistics.median(c for _, c, _ in samples[config])
    sw = statistics.median(s for _, _, s in samples[config])
    forks, limit = config
    print(f"{forks:>6} {limit or '-':>6} {wall:>8.2f} {wall / base:>5.2f}x {spread:>6.0%} {cpu:>7.2f} {sw:>14.0f}")
