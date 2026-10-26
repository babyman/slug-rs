"""Grade a Python-arm game module: play each case's keys and compare every
screen it shows, and its final snapshot, with the reference's.

The module (python/cases.json's `module`) defines `new_game(seed)`,
`load(snapshot)`, `press(game, key, shift)`, `screen(game)`, a list of
strings, and `snapshot(game)`, a JSON-able dict. Its directory's `.py` files
are copied whole, then the project's python/workspace files over them, so
provided data cannot be changed. A case starts with `{"seed": n}` or
`{"load": snapshot}`, presses `keys` ([key, shift] pairs), and passes when
the screen before the first key and after every key matches `screens` (the
first 16 hex digits of the SHA-256 of the rows joined with newlines) and the
final snapshot matches `snapshot`.
"""

import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from arcade_grader import first_difference

# Runs one case in its own process and writes what it saw to result.json, so
# the module's own printing cannot get in the way.
DRIVER = """\
import hashlib, json, sys
sys.path.insert(0, ".")
case = json.load(open("case.json"))
import {module} as game_module

def digest(game):
    rows = game_module.screen(game)
    if type(rows) is not list or len(rows) != 24 or any(type(row) is not str for row in rows):
        raise SystemExit("screen() did not return a list of 24 strings: " + repr(rows)[:200])
    return hashlib.sha256("\\n".join(rows).encode()).hexdigest()[:16], rows

if "seed" in case["start"]:
    game = game_module.new_game(case["start"]["seed"])
else:
    game = game_module.load(case["start"]["load"])
screens, shown = [], []
for i in range(len(case["keys"]) + 1):
    if i > 0:
        key, shift = case["keys"][i - 1]
        game_module.press(game, key, shift)
    found, rows = digest(game)
    screens.append(found)
    if found != case["screens"][i]:
        shown = rows
        break
snapshot = json.loads(json.dumps(game_module.snapshot(game)))
json.dump({{"screens": screens, "shown": shown, "snapshot": snapshot}}, open("result.json", "w"))
"""


def describe(key, shift):
    return f"shift {key}" if shift else key


def grade(python, project_dir, module_path, suite):
    module = suite["module"]
    outcomes = []
    for case in suite["cases"]:
        outcomes.append({"id": case["id"], "kind": "python", **run_case(python, project_dir, module_path, module, case)})
    return outcomes


def run_case(python, project_dir, module_path, module, case):
    with tempfile.TemporaryDirectory(prefix="slug-eval-python-game-") as directory:
        directory = Path(directory)
        for source in Path(module_path).parent.glob("*.py"):
            shutil.copyfile(source, directory / source.name)
        provided = Path(project_dir) / "python" / "workspace"
        if provided.is_dir():
            for source in provided.iterdir():
                if source.is_file():
                    shutil.copyfile(source, directory / source.name)
        shutil.copyfile(module_path, directory / f"{module}.py")
        (directory / "case.json").write_text(json.dumps(case))
        (directory / "driver.py").write_text(DRIVER.format(module=module))
        try:
            completed = subprocess.run(
                [python, "-I", "driver.py"],
                cwd=directory,
                capture_output=True,
                text=True,
                timeout=case["seconds"],
                env={key: value for key, value in os.environ.items() if not key.startswith("PYTHON")},
            )
        except subprocess.TimeoutExpired:
            return {"passed": False, "detail": f"timed out after {case['seconds']} s"}
        result_file = directory / "result.json"
        if completed.returncode != 0 or not result_file.exists():
            return {"passed": False, "detail": (completed.stderr.strip() or completed.stdout.strip())[-300:] or f"exit {completed.returncode}"}
        result = json.loads(result_file.read_text())
    screens = result["screens"]
    if screens != case["screens"]:
        i = len(screens) - 1
        when = "the first screen" if i == 0 else f"the screen after key {i} ({describe(*case['keys'][i - 1])})"
        shown = result["shown"]
        return {"passed": False, "detail": f"{when} differs; its message line is `{shown[0].rstrip()[:100]}`, status line `{shown[23].rstrip()[:100]}`"}
    state = first_difference(result["snapshot"], case["snapshot"])
    if state:
        return {"passed": False, "detail": f"final state: {state}"}
    return {"passed": True, "detail": ""}
