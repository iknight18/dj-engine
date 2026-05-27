from __future__ import annotations

import json
from pathlib import Path

from dj_analyze.pipeline import run_pipeline

FIXTURES = Path(__file__).parent / "fixtures"


def test_pipeline_writes_one_sidecar_per_track(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    # Use a tiny set of fixtures (skip the slow ones for this test).
    for name in ("click_120bpm.wav",):
        (source / name).write_bytes((FIXTURES / name).read_bytes())

    output = tmp_path / "out"
    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1
    )
    assert n_processed == 1
    assert n_skipped == 0
    assert n_errors == 0
    sidecar = output / "click_120bpm.wav.json"
    assert sidecar.exists()
    data = json.loads(sidecar.read_text())
    assert data["schema_version"] == 2


def test_pipeline_is_idempotent(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click_120bpm.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())
    output = tmp_path / "out"

    run_pipeline(source_dir=source, output_dir=output, workers=1)
    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1
    )
    assert n_processed == 0
    assert n_skipped == 1
    assert n_errors == 0


def test_pipeline_force_reanalyzes(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click_120bpm.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())
    output = tmp_path / "out"

    run_pipeline(source_dir=source, output_dir=output, workers=1)
    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1, force=True
    )
    assert n_processed == 1
    assert n_skipped == 0


def test_pipeline_records_error_for_corrupt_file(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "broken.mp3").write_bytes(b"\x00")
    output = tmp_path / "out"

    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1
    )
    assert n_errors == 1
    data = json.loads((output / "broken.mp3.json").read_text())
    assert "error" in data
