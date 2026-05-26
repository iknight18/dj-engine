from __future__ import annotations

from pathlib import Path

import pytest

from dj_analyze.io_helpers import discover_audio_files, sidecar_path_for


AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".wav", ".ogg", ".opus"}


def test_discover_finds_supported_extensions(tmp_path: Path) -> None:
    (tmp_path / "a.mp3").write_bytes(b"")
    (tmp_path / "sub").mkdir()
    (tmp_path / "sub" / "b.flac").write_bytes(b"")
    (tmp_path / "ignore.txt").write_bytes(b"")

    found = sorted(p.name for p in discover_audio_files(tmp_path))
    assert found == ["a.mp3", "b.flac"]


def test_discover_is_recursive(tmp_path: Path) -> None:
    deep = tmp_path / "a" / "b" / "c"
    deep.mkdir(parents=True)
    (deep / "deep.wav").write_bytes(b"")

    found = list(discover_audio_files(tmp_path))
    assert len(found) == 1
    assert found[0].name == "deep.wav"


def test_discover_is_case_insensitive(tmp_path: Path) -> None:
    (tmp_path / "UPPER.MP3").write_bytes(b"")
    found = list(discover_audio_files(tmp_path))
    assert len(found) == 1


def test_sidecar_path_mirrors_relative_path(tmp_path: Path) -> None:
    source_root = tmp_path / "music"
    output_root = tmp_path / "out"
    track = source_root / "Artist" / "Album" / "01 - Track.mp3"
    source_root.mkdir()
    track.parent.mkdir(parents=True)
    track.write_bytes(b"")

    sc = sidecar_path_for(track, source_root, output_root)
    assert sc == output_root / "Artist" / "Album" / "01 - Track.mp3.json"


def test_sidecar_path_handles_track_with_dots(tmp_path: Path) -> None:
    source_root = tmp_path / "music"
    source_root.mkdir()
    track = source_root / "1.2.3 - Mix.flac"
    track.write_bytes(b"")
    sc = sidecar_path_for(track, source_root, tmp_path / "out")
    assert sc.name == "1.2.3 - Mix.flac.json"


def test_sidecar_path_raises_if_not_under_root(tmp_path: Path) -> None:
    other = tmp_path / "other.mp3"
    other.write_bytes(b"")
    with pytest.raises(ValueError):
        sidecar_path_for(other, tmp_path / "music", tmp_path / "out")
