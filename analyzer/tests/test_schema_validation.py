from __future__ import annotations

import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator

from dj_analyze.analyze import analyze_track

FIXTURES = Path(__file__).parent / "fixtures"
SCHEMA_PATH = Path(__file__).parents[2] / "schema" / "sidecar.schema.json"


@pytest.fixture(scope="module")
def validator() -> Draft202012Validator:
    schema = json.loads(SCHEMA_PATH.read_text())
    return Draft202012Validator(schema)


def test_v2_sidecar_validates(validator: Draft202012Validator) -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_corrupt_sidecar_validates(validator: Draft202012Validator) -> None:
    """Error sidecars carry only the base fields + error."""
    result = analyze_track(FIXTURES / "corrupt.mp3")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_bass_only_sidecar_validates(validator: Draft202012Validator) -> None:
    result = analyze_track(FIXTURES / "bass_only_80hz.wav")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_legacy_v1_minimal_sidecar_validates_via_only_required_fields(
    validator: Draft202012Validator,
) -> None:
    """A v1-style sidecar that happens to set schema_version=2 should still validate
    as long as required fields are present; new fields are optional."""
    minimal = {
        "schema_version": 2,
        "analyzer_version": "0.1.0",
        "source_path": "x",
        "source_mtime": 0.0,
        "source_size_bytes": 0,
        "duration_seconds": 200.0,
        "sample_rate": 44100,
        "bpm": 120.0,
        "bpm_confidence": 0.9,
        "beats_seconds": [0.0],
        "downbeats_seconds": [0.0],
        "key": "8A",
        "key_label": "A minor",
        "key_confidence": 0.8,
        "energy_curve": [0.4],
        "energy_mean": 0.4,
        "cue_in_seconds": 0.0,
        "cue_out_seconds": 195.0,
        "intro_end_seconds": 16.0,
        "outro_start_seconds": 180.0,
        "is_mixable": True,
        "mixable_reasons": [],
    }
    errors = list(validator.iter_errors(minimal))
    assert not errors, "Minimal-fields sidecar should validate:\n" + "\n".join(
        e.message for e in errors
    )
