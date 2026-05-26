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


def test_click_sidecar_validates(validator: Draft202012Validator) -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_corrupt_sidecar_validates(validator: Draft202012Validator) -> None:
    """Error sidecars only have base fields + `error`; must still validate."""
    result = analyze_track(FIXTURES / "corrupt.mp3")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)
