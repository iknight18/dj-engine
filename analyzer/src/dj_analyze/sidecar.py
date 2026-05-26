"""Sidecar JSON load/save with atomic writes, and freshness check (skip-if-fresh)."""

from __future__ import annotations

import json
import os
from pathlib import Path
from tempfile import NamedTemporaryFile
from typing import Any


def save_sidecar(path: Path, data: dict[str, Any]) -> None:
    """Write `data` to `path` as JSON atomically.

    Writes to a temp file in the same directory, then renames over the target.
    If serialization fails, the target file is left untouched.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    # Use a temp file in the same dir so the rename is atomic across the same filesystem.
    with NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        dir=path.parent,
        delete=False,
        suffix=".tmp",
    ) as tmp:
        tmp_path = Path(tmp.name)
        try:
            json.dump(data, tmp, ensure_ascii=False, indent=2)
        except Exception:
            tmp.close()
            tmp_path.unlink(missing_ok=True)
            raise
    os.replace(tmp_path, path)


def load_sidecar(path: Path) -> dict[str, Any]:
    """Load a sidecar JSON file. Raises on missing or invalid JSON."""
    with path.open("r", encoding="utf-8") as f:
        result: dict[str, Any] = json.load(f)
    return result


def is_fresh(track: Path, sidecar_path: Path, analyzer_version: str) -> bool:
    """Return True if `sidecar_path` represents a current analysis of `track`.

    Fresh means: file exists, valid JSON, and source_mtime + source_size_bytes
    + analyzer_version all match.
    """
    if not sidecar_path.exists():
        return False
    try:
        data = load_sidecar(sidecar_path)
    except (json.JSONDecodeError, OSError):
        return False
    if data.get("analyzer_version") != analyzer_version:
        return False
    stat = track.stat()
    if data.get("source_size_bytes") != stat.st_size:
        return False
    # mtime comparison: tolerate 1ms drift (filesystem timestamp granularity)
    sidecar_mtime = data.get("source_mtime")
    if not isinstance(sidecar_mtime, (int, float)):
        return False
    if abs(sidecar_mtime - stat.st_mtime) > 0.001:
        return False
    return True
