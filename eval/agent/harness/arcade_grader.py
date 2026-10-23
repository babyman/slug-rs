"""Grade a slug.arcade game: run it headless on each case's input script and
compare every frame it shows with the reference's.

The game's directory is copied whole (its `.slug` files), then the project's
`workspace/` files are copied over it, so provided data cannot be changed.
Each case runs the program with SLUG_ARCADE_HEADLESS set to the case's script
and SLUG_ARCADE_HASHES collecting each frame's 64-bit hash. Frames are
compared in windows of `window` frames: a window's digest is the first 16 hex
digits of the SHA-256 of its frames' hashes, and cases.json stores each
case's digests. A case passes when the program shows exactly `frames` frames
and every digest matches, and its sound log (SLUG_ARCADE_SOUNDS: one line
per play, loop, or stop call) has exactly the case's `sounds` lines.
"""

import hashlib
import os
import shutil
import subprocess
import tempfile
from pathlib import Path


def digests(hashes, window):
    """Window digests of a run's frame hashes, 8 bytes per frame."""
    frames = len(hashes) // 8
    return [
        hashlib.sha256(hashes[start * 8 : min(start + window, frames) * 8]).hexdigest()[:16]
        for start in range(0, frames, window)
    ]


def run(slug, slug_home, project_dir, program_path, script, seconds):
    """Runs a program on an input script and returns (hashes, sound lines,
    error)."""
    with tempfile.TemporaryDirectory(prefix="slug-eval-arcade-") as directory:
        directory = Path(directory)
        for source in Path(program_path).parent.glob("*.slug"):
            shutil.copyfile(source, directory / source.name)
        provided = Path(project_dir) / "workspace"
        if provided.is_dir():
            for source in provided.iterdir():
                if source.is_file():
                    shutil.copyfile(source, directory / source.name)
        program = directory / Path(program_path).name
        shutil.copyfile(program_path, program)
        (directory / "script.txt").write_text(script)
        hashes = directory / "hashes.bin"
        sounds = directory / "sounds.txt"
        environment = {
            **os.environ,
            "SLUG_HOME": str(slug_home),
            "SLUG_ARCADE_HEADLESS": str(directory / "script.txt"),
            "SLUG_ARCADE_HASHES": str(hashes),
            "SLUG_ARCADE_SOUNDS": str(sounds),
            "SLUG_ARCADE_OUT": str(directory),
        }
        try:
            completed = subprocess.run(
                [slug, program.name],
                cwd=directory,
                capture_output=True,
                text=True,
                timeout=seconds,
                env=environment,
            )
        except subprocess.TimeoutExpired:
            return None, None, f"timed out after {seconds} s"
        if completed.returncode != 0:
            return None, None, (completed.stderr.strip() or completed.stdout.strip())[:300] or f"exit {completed.returncode}"
        if not hashes.exists():
            return None, None, "no frames shown (was slug.arcade opened and closed?)"
        lines = sounds.read_text().splitlines() if sounds.exists() else []
        return hashes.read_bytes(), lines, None


def grade(slug, slug_home, project_dir, program_path, suite):
    window = suite["window"]
    outcomes = []
    for case in suite["cases"]:
        hashes, sounds, error = run(slug, slug_home, project_dir, program_path, case["script"], case["seconds"])
        if error is not None:
            outcomes.append({"id": case["id"], "kind": "arcade", "passed": False, "detail": error})
            continue
        frames = len(hashes) // 8
        got = digests(hashes, window)
        expected = case["windows"]
        mismatch = next((i for i, (a, b) in enumerate(zip(got, expected)) if a != b), None)
        if mismatch is not None:
            detail = f"frames {mismatch * window}-{mismatch * window + window - 1} differ"
        elif frames != case["frames"]:
            detail = f"showed {frames} frames, expected {case['frames']}"
        elif sounds != case["sounds"]:
            line = next((i for i, (a, b) in enumerate(zip(sounds, case["sounds"])) if a != b), min(len(sounds), len(case["sounds"])))
            got = sounds[line] if line < len(sounds) else "nothing"
            wanted = case["sounds"][line] if line < len(case["sounds"]) else "nothing"
            detail = f"sound call {line + 1} is `{got}`, expected `{wanted}`"
        else:
            detail = ""
        outcomes.append({"id": case["id"], "kind": "arcade", "passed": detail == "", "detail": detail})
    return outcomes
