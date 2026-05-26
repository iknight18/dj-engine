"""CLI entry: `dj-analyze SOURCE_DIR OUTPUT_DIR [--workers N] [--force]`."""

from __future__ import annotations

import argparse
import logging
import os
import sys
from pathlib import Path

from .logging_config import configure_logging
from .pipeline import run_pipeline

log = logging.getLogger("dj_analyze.cli")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="dj-analyze",
        description="Analyze a directory of audio files and write sidecar JSONs.",
    )
    parser.add_argument(
        "source_dir", type=Path, help="Root directory of audio files (recursive)"
    )
    parser.add_argument(
        "output_dir", type=Path, help="Where to write sidecar JSONs"
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=max(1, (os.cpu_count() or 2) - 1),
        help="Parallel workers (default: CPU count - 1)",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-analyze tracks even if a fresh sidecar exists",
    )
    parser.add_argument(
        "--log-format",
        choices=("pretty", "json"),
        default="pretty",
        help="Log output style (default: pretty)",
    )
    args = parser.parse_args(argv)

    configure_logging(args.log_format)

    try:
        processed, skipped, errors = run_pipeline(
            source_dir=args.source_dir,
            output_dir=args.output_dir,
            workers=args.workers,
            force=args.force,
        )
    except FileNotFoundError as e:
        log.error("input error: %s", e)
        return 2

    log.info(
        "done",
        extra={"processed": processed, "skipped": skipped, "errors": errors},
    )
    return 0 if errors == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
