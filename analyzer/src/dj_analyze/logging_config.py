"""Configure logging — `pretty` (default) or `json` (for production / Rancher)."""

from __future__ import annotations

import json
import logging
import sys
from typing import Literal


class _JsonFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        payload: dict[str, object] = {
            "ts": self.formatTime(record, datefmt="%Y-%m-%dT%H:%M:%S"),
            "level": record.levelname,
            "logger": record.name,
            "msg": record.getMessage(),
        }
        # Propagate any `extra=` fields passed by callers.
        reserved = {
            "name", "msg", "args", "levelname", "levelno", "pathname", "filename",
            "module", "exc_info", "exc_text", "stack_info", "lineno", "funcName",
            "created", "msecs", "relativeCreated", "thread", "threadName",
            "processName", "process", "message", "taskName",
        }
        for k, v in record.__dict__.items():
            if k not in reserved:
                payload[k] = v
        if record.exc_info:
            payload["exc"] = self.formatException(record.exc_info)
        return json.dumps(payload, ensure_ascii=False, default=str)


def configure_logging(format_: Literal["pretty", "json"] = "pretty") -> None:
    """Reset and configure the root logger for the analyzer process."""
    root = logging.getLogger()
    root.handlers.clear()
    root.setLevel(logging.INFO)

    handler = logging.StreamHandler(sys.stderr)
    if format_ == "json":
        handler.setFormatter(_JsonFormatter())
    else:
        handler.setFormatter(
            logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
        )
    root.addHandler(handler)
