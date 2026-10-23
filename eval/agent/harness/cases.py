"""Run a Slug solution against task cases in isolated directories."""

import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

ENTRY = "solution.slug"
CASE_TIMEOUT_SECONDS = 20


def toml_value(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(item) for item in value) + "]"
    raise TypeError(f"unsupported configuration value: {value!r}")


def case_toml(config):
    """A slug.toml giving `cfg` keys local to the `solution` entry module."""
    lines = ["[solution]"]
    lines += [f"{key} = {toml_value(value)}" for key, value in config.items()]
    return "\n".join(lines) + "\n"


def load_cases(task_dir):
    return json.loads((Path(task_dir) / "cases.json").read_text())["cases"]


def run_case(slug, slug_home, source_path, case):
    """Returns (exit code, stdout, stderr) for one case run in a fresh directory."""
    with tempfile.TemporaryDirectory(prefix="slug-eval-case-") as directory:
        shutil.copyfile(source_path, os.path.join(directory, ENTRY))
        config = {key: value for key, value in case.items() if key != "stdin"}
        if config:
            Path(directory, "slug.toml").write_text(case_toml(config))
        try:
            completed = subprocess.run(
                [slug, ENTRY],
                cwd=directory,
                input=case.get("stdin", ""),
                capture_output=True,
                text=True,
                timeout=CASE_TIMEOUT_SECONDS,
                env={**os.environ, "SLUG_HOME": str(slug_home)},
            )
        except subprocess.TimeoutExpired:
            return (None, "", "timed out")
        return (completed.returncode, completed.stdout, completed.stderr)
