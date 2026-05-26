"""Audio file discovery and sidecar output-path computation. Pure functions only."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

AUDIO_EXTENSIONS: frozenset[str] = frozenset(
    {".mp3", ".flac", ".m4a", ".wav", ".ogg", ".opus"}
)


def discover_audio_files(root: Path) -> Iterator[Path]:
    """Yield every audio file under `root` (recursive). Case-insensitive on extension."""
    for path in root.rglob("*"):
        if path.is_file() and path.suffix.lower() in AUDIO_EXTENSIONS:
            yield path


def sidecar_path_for(track: Path, source_root: Path, output_root: Path) -> Path:
    """Compute the sidecar JSON path that mirrors `track`'s relative position under `source_root`.

    The sidecar filename is `<track filename>.json` (preserves extension to disambiguate
    `song.mp3` from `song.flac`).
    """
    try:
        relative = track.resolve().relative_to(source_root.resolve())
    except ValueError as e:
        raise ValueError(
            f"Track {track} is not inside source root {source_root}"
        ) from e
    return output_root / relative.with_name(relative.name + ".json")
