#!/usr/bin/env python3
"""Plays seeded games with a bot on the Python reference, replays the keys on
the Slug reference headless, and compares the final states and screens.

Usage: compare_references.py <games> <commands per game>
       compare_references.py find <seed> <commands>   (first differing command)
The slug binary is $SLUG, or the repository's release build.
"""
import json, os, random, subprocess, sys, tempfile
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(HERE))))
sys.path.insert(0, HERE + "/tools")
from bot import play, script, pygame_  # noqa: E402

slug = os.environ.get("SLUG", REPO + "/target/release/slug")
env = dict(os.environ, SLUG_HOME=REPO)
def slug_state(seed, keys, tmp):
    open(f"{tmp}/s.txt", "w").write(script(keys))
    subprocess.run([slug, "crawl.slug", "--seed", str(seed), "--dump", f"{tmp}/d.json", "--screen", f"{tmp}/t.json"],
                   cwd=HERE + "/reference", env=dict(env, SLUG_ARCADE_HEADLESS=f"{tmp}/s.txt", SLUG_ARCADE_OUT=tmp),
                   check=True)
    return dict(json.load(open(f"{tmp}/d.json")), text=json.load(open(f"{tmp}/t.json")))

def python_state(seed, keys):
    g = pygame_.Game(seed)
    for key, shift in keys:
        g.command(key, shift)
    return json.loads(json.dumps(dict(g.state(), text=g.text())))

if sys.argv[1] == "find":
    # find <seed> <commands>: the first command after which the two differ.
    seed, commands = int(sys.argv[2]), int(sys.argv[3])
    _, keys = play(seed, commands, random.Random(seed))
    with tempfile.TemporaryDirectory() as tmp:
        lo, hi = 0, len(keys)  # states agree after lo commands, differ after hi
        while hi - lo > 1:
            mid = (lo + hi) // 2
            if slug_state(seed, keys[:mid], tmp) == python_state(seed, keys[:mid]):
                lo = mid
            else:
                hi = mid
        before = python_state(seed, keys[:lo])
        got, want = slug_state(seed, keys[:hi], tmp), python_state(seed, keys[:hi])
        print(f"command {hi}: {keys[hi - 1]}")
        print("before:", "\n".join(before["text"][:1] + before["text"][23:]))
        print("slug:  ", got["text"][0], "|", got["text"][23])
        print("python:", want["text"][0], "|", want["text"][23])
        for k in want:
            if got.get(k) != want[k] and k not in ("text", "memory", "map"):
                print(k, "slug:", json.dumps(got.get(k))[:300])
                print(k, "py:  ", json.dumps(want[k])[:300])
    sys.exit(0)

seeds, commands = int(sys.argv[1]), int(sys.argv[2])
ok = 0
with tempfile.TemporaryDirectory() as tmp:
    first = int(os.environ.get("FIRST", 1))
    for seed in range(first, first + seeds):
        g, keys = play(seed, commands, random.Random(seed))
        got = slug_state(seed, keys, tmp)
        want = json.loads(json.dumps(dict(g.state(), text=g.text())))
        diff = [k for k in want if got.get(k) != want[k]] + [k for k in got if k not in want]
        if diff:
            print("seed", seed, "differs in", diff)
        else:
            ok += 1
        print(f"seed {seed}: seed now {g.seed} depth {g.depth} turn {g.turn} xp {g.player['xp']}", file=sys.stderr)
print(ok, "of", seeds, "identical")
