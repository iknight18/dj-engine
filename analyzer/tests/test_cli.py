from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

FIXTURES = Path(__file__).parent / "fixtures"


def _run_cli(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-m", "dj_analyze", *args],
        capture_output=True,
        text=True,
        cwd=cwd,
        env={**os.environ},
    )


def test_cli_help_exits_zero() -> None:
    result = _run_cli("--help")
    assert result.returncode == 0
    assert "dj-analyze" in result.stdout.lower() or "SOURCE_DIR" in result.stdout


def test_cli_analyzes_directory(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())

    output = tmp_path / "analyzed"
    result = _run_cli(str(source), str(output), "--workers", "1")
    assert result.returncode == 0, result.stderr

    sidecar = output / "click.wav.json"
    assert sidecar.exists()
    assert json.loads(sidecar.read_text())["schema_version"] == 1


def test_cli_json_log_mode_emits_json(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())
    output = tmp_path / "analyzed"

    result = _run_cli(
        str(source), str(output), "--workers", "1", "--log-format", "json"
    )
    assert result.returncode == 0
    # At least one stderr line should parse as JSON.
    json_lines = [
        line for line in result.stderr.splitlines() if line.startswith("{")
    ]
    assert json_lines, f"No JSON log lines found in stderr: {result.stderr!r}"
    parsed = json.loads(json_lines[0])
    assert "level" in parsed
