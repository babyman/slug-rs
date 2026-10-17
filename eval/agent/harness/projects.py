"""Grade a Slug project module against its hidden cases.

Each case runs as its own program beside the agent's module, so a crash,
wrong error, or infinite loop fails only that case. Inputs and expected JSON
text reach the program through `cfg`, read from a slug.toml, so they need no
escaping into Slug source.
"""

import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

CASE_TIMEOUT_SECONDS = 20

PROGRAMS = {
    "parse": """\
val json = import("{module}")
val expected = {expect}
val check = fn() {{
  defer onerror(err) {{ println("FAIL threw", err) }}
  val actual = json.parse(cfg("input", ""))
  if (actual == expected) {{ println("PASS") }} else {{ println("FAIL got", actual) }}
}}
check()
""",
    "parse_error": """\
val json = import("{module}")
val check = fn() {{
  defer onerror(err) {{
    match err {{
      e if e.type == "json.parse"{offset} => println("PASS")
      e => println("FAIL wrong error", e)
    }}
  }}
  val actual = json.parse(cfg("input", ""))
  println("FAIL parsed", actual)
}}
check()
""",
    "stringify": """\
val json = import("{module}")
{setup}
val check = fn() {{
  defer onerror(err) {{ println("FAIL threw", err) }}
  val actual = json.stringify({value})
  if (actual == cfg("expect", "")) {{ println("PASS") }} else {{ println("FAIL got", actual) }}
}}
check()
""",
    "stringify_error": """\
val json = import("{module}")
{setup}
val check = fn() {{
  defer onerror(err) {{
    match err {{
      e if e.type == "json.type" => println("PASS")
      e => println("FAIL wrong error", e)
    }}
  }}
  val actual = json.stringify({value})
  println("FAIL wrote", actual)
}}
check()
""",
    "roundtrip": """\
val json = import("{module}")
val check = fn() {{
  defer onerror(err) {{ println("FAIL threw", err) }}
  val actual = json.stringify(json.parse(cfg("input", "")))
  if (actual == cfg("expect", "")) {{ println("PASS") }} else {{ println("FAIL got", actual) }}
}}
check()
""",
}


def load_cases(project_dir):
    return json.loads((Path(project_dir) / "cases.json").read_text())["cases"]


def case_program(case, module):
    offset = case.get("offset")
    return PROGRAMS[case["kind"]].format(
        module=module,
        expect=case.get("expect", "nil"),
        value=case.get("value", "nil"),
        setup=case.get("setup", ""),
        offset="" if offset is None else f" && e.data.offset == {offset}",
    )


def case_toml(case):
    # JSON string escapes are valid TOML basic-string escapes; keeping
    # non-ASCII text literal avoids surrogate escapes TOML rejects.
    lines = ["[case]"]
    if "input" in case:
        lines.append(f"input = {json.dumps(case['input'], ensure_ascii=False)}")
    if case["kind"] in ("stringify", "roundtrip"):
        lines.append(f"expect = {json.dumps(case['expect'], ensure_ascii=False)}")
    return "\n".join(lines) + "\n"


def run_case(slug, slug_home, module_path, module, case):
    """Returns (passed, detail) for one case."""
    with tempfile.TemporaryDirectory(prefix="slug-eval-project-") as directory:
        shutil.copyfile(module_path, os.path.join(directory, f"{module}.slug"))
        Path(directory, "case.slug").write_text(case_program(case, module))
        Path(directory, "slug.toml").write_text(case_toml(case))
        try:
            completed = subprocess.run(
                [slug, "case.slug"],
                cwd=directory,
                capture_output=True,
                text=True,
                timeout=CASE_TIMEOUT_SECONDS,
                env={**os.environ, "SLUG_HOME": str(slug_home)},
            )
        except subprocess.TimeoutExpired:
            return False, "timed out"
        passed = completed.returncode == 0 and completed.stdout.strip() == "PASS"
        detail = (completed.stdout.strip() or completed.stderr.strip())[:300]
        return passed, detail


def grade(slug, slug_home, project_dir, module_path, module):
    """Runs every case and returns per-case outcomes."""
    outcomes = []
    for case in load_cases(project_dir):
        passed, detail = run_case(slug, slug_home, module_path, module, case)
        outcomes.append({"id": case["id"], "kind": case["kind"], "passed": passed, "detail": detail})
    return outcomes
