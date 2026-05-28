"""Parallel pipeline runner: scan a directory, analyze each track, write sidecars."""

from __future__ import annotations

import logging
from collections.abc import Callable
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from rich.progress import (
    BarColumn,
    MofNCompleteColumn,
    Progress,
    SpinnerColumn,
    TextColumn,
    TimeRemainingColumn,
)

from . import __version__
from .analyze import analyze_track
from .io_helpers import discover_audio_files, sidecar_path_for
from .sidecar import is_fresh, load_sidecar, save_sidecar

log = logging.getLogger("dj_analyze.pipeline")


def _analyze_one(args: tuple[Path, Path, Path | None]) -> tuple[Path, dict[str, Any]]:
    """Worker entry point — must be top-level for multiprocessing pickling."""
    track, sidecar, stems_root = args
    return sidecar, analyze_track(track, stems_root=stems_root)


def run_pipeline(
    *,
    source_dir: Path,
    output_dir: Path,
    workers: int = 1,
    force: bool = False,
) -> tuple[int, int, int]:
    """Walk `source_dir`, analyze tracks, write sidecars under `output_dir`.

    Returns (n_processed, n_skipped, n_errors).
    """
    if not source_dir.exists() or not source_dir.is_dir():
        raise FileNotFoundError(f"Source dir not found: {source_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)

    # Per spec: stems land alongside the sidecar dir under "<output_dir>-stems".
    # e.g. analyzed-real -> analyzed-real-stems
    stems_root = output_dir.parent / f"{output_dir.name}-stems"
    stems_root.mkdir(parents=True, exist_ok=True)

    tracks = list(discover_audio_files(source_dir))
    todo: list[tuple[Path, Path]] = []
    n_skipped = 0
    for track in tracks:
        sidecar = sidecar_path_for(track, source_dir, output_dir)
        if not force and is_fresh(track, sidecar, __version__):
            n_skipped += 1
            continue
        todo.append((track, sidecar))

    n_processed = 0
    n_errors = 0

    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        MofNCompleteColumn(),
        TimeRemainingColumn(),
    ) as progress:
        task = progress.add_task("Analyzing tracks", total=len(todo))

        if workers <= 1:
            for track, sidecar in todo:
                _process_one(track, sidecar, stems_root=stems_root, on_error=lambda: None)
                data = load_sidecar(sidecar)
                if "error" in data:
                    n_errors += 1
                else:
                    n_processed += 1
                progress.advance(task)
        else:
            with ProcessPoolExecutor(max_workers=workers) as pool:
                futures = {
                    pool.submit(_analyze_one, (track, sidecar, stems_root)): (track, sidecar)
                    for track, sidecar in todo
                }
                for future in as_completed(futures):
                    sidecar, data = future.result()
                    save_sidecar(sidecar, data)
                    if "error" in data:
                        n_errors += 1
                    else:
                        n_processed += 1
                    progress.advance(task)

    log.info(
        "pipeline complete",
        extra={
            "processed": n_processed,
            "skipped": n_skipped,
            "errors": n_errors,
        },
    )
    return n_processed, n_skipped, n_errors


def _process_one(
    track: Path, sidecar: Path, *, stems_root: Path | None, on_error: Callable[[], None]
) -> None:
    """Single-worker codepath: analyze and save in this process."""
    data = analyze_track(track, stems_root=stems_root)
    save_sidecar(sidecar, data)
