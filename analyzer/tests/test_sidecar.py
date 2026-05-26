from __future__ import annotations

import json
from pathlib import Path

from dj_analyze.sidecar import is_fresh, load_sidecar, save_sidecar


def _make_track(tmp_path: Path, contents: bytes = b"audio") -> Path:
    track = tmp_path / "song.mp3"
    track.write_bytes(contents)
    return track


def test_save_writes_valid_json(tmp_path: Path) -> None:
    out = tmp_path / "out.json"
    save_sidecar(out, {"schema_version": 1, "analyzer_version": "0.1.0"})
    loaded = json.loads(out.read_text())
    assert loaded["schema_version"] == 1


def test_save_is_atomic_no_partial_file(tmp_path: Path, monkeypatch) -> None:
    """If serialization fails, no broken file should exist at the target path."""
    out = tmp_path / "out.json"
    out.write_text("OLD")

    class Unserializable:
        pass

    try:
        save_sidecar(out, {"bad": Unserializable()})
    except TypeError:
        pass
    # The old content (or no file) should remain — not a half-written one.
    assert out.read_text() == "OLD"


def test_load_returns_dict(tmp_path: Path) -> None:
    out = tmp_path / "out.json"
    out.write_text('{"hello": "world"}')
    assert load_sidecar(out) == {"hello": "world"}


def test_is_fresh_true_when_mtime_size_version_match(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    sidecar_path = tmp_path / "song.mp3.json"
    save_sidecar(
        sidecar_path,
        {
            "schema_version": 1,
            "analyzer_version": "0.1.0",
            "source_mtime": track.stat().st_mtime,
            "source_size_bytes": track.stat().st_size,
        },
    )
    assert is_fresh(track, sidecar_path, "0.1.0")


def test_is_fresh_false_when_size_changed(tmp_path: Path) -> None:
    track = _make_track(tmp_path, b"audio")
    sidecar_path = tmp_path / "song.mp3.json"
    save_sidecar(
        sidecar_path,
        {
            "schema_version": 1,
            "analyzer_version": "0.1.0",
            "source_mtime": track.stat().st_mtime,
            "source_size_bytes": 999,  # wrong
        },
    )
    assert not is_fresh(track, sidecar_path, "0.1.0")


def test_is_fresh_false_when_analyzer_version_changed(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    sidecar_path = tmp_path / "song.mp3.json"
    save_sidecar(
        sidecar_path,
        {
            "schema_version": 1,
            "analyzer_version": "0.1.0",
            "source_mtime": track.stat().st_mtime,
            "source_size_bytes": track.stat().st_size,
        },
    )
    assert not is_fresh(track, sidecar_path, "0.2.0")


def test_is_fresh_false_when_sidecar_missing(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    assert not is_fresh(track, tmp_path / "missing.json", "0.1.0")


def test_is_fresh_false_when_sidecar_corrupt(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    sidecar_path = tmp_path / "broken.json"
    sidecar_path.write_text("not valid json {")
    assert not is_fresh(track, sidecar_path, "0.1.0")
