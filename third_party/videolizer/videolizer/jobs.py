"""
Per-job folder creation, logging, and structured event tracking.

Every videolizer job gets:
- job_dir/
  - job.json       (input plan)
  - result.json    (output result)
  - logs.txt       (human-readable logs)
  - events.jsonl   (structured events for debugging)
  - audio/, images/, video/, etc. (artifacts)
"""

import json
import logging
from datetime import datetime
from pathlib import Path
from typing import Any


class JobLogger:
    """
    Dual logging: human-readable logs.txt + structured events.jsonl.
    When something breaks, check job_dir/logs.txt and events.jsonl.
    """

    def __init__(self, job_dir: Path):
        self.job_dir = Path(job_dir)
        self.job_dir.mkdir(parents=True, exist_ok=True)
        self._log_path = self.job_dir / "logs.txt"
        self._events_path = self.job_dir / "events.jsonl"
        self._file_handle = open(self._log_path, "a", encoding="utf-8")
        self._events_handle = open(self._events_path, "a", encoding="utf-8")
        self._logger = self._setup_logger()

    def _setup_logger(self) -> logging.Logger:
        logger = logging.getLogger(f"videolizer.{self.job_dir.name}")
        logger.setLevel(logging.DEBUG)
        logger.handlers.clear()
        h = logging.StreamHandler(self._file_handle)
        h.setFormatter(
            logging.Formatter("%(asctime)s [%(levelname)s] %(name)s: %(message)s")
        )
        logger.addHandler(h)
        return logger

    def info(self, msg: str, **kwargs: Any) -> None:
        self._logger.info(msg)
        self._event("info", msg, **kwargs)

    def warning(self, msg: str, **kwargs: Any) -> None:
        self._logger.warning(msg)
        self._event("warning", msg, **kwargs)

    def error(self, msg: str, **kwargs: Any) -> None:
        self._logger.error(msg)
        self._event("error", msg, **kwargs)

    def debug(self, msg: str, **kwargs: Any) -> None:
        self._logger.debug(msg)
        self._event("debug", msg, **kwargs)

    def _event(self, level: str, message: str, **extra: Any) -> None:
        record = {
            "ts": datetime.utcnow().isoformat() + "Z",
            "level": level,
            "message": message,
            **extra,
        }
        self._events_handle.write(json.dumps(record, ensure_ascii=False) + "\n")
        self._events_handle.flush()

    def step_start(self, step: str, **extra: Any) -> None:
        self._event("step_start", step, step=step, **extra)

    def step_end(self, step: str, duration_s: float, **extra: Any) -> None:
        self._event("step_end", step, step=step, duration_s=duration_s, **extra)

    def close(self) -> None:
        self._file_handle.close()
        self._events_handle.close()


def create_job_dir(base_dir: Path, job_id: str) -> Path:
    """Create per-job directory with standard subdirs."""
    job_dir = base_dir / job_id
    job_dir.mkdir(parents=True, exist_ok=True)
    for sub in ("audio", "images", "video", "music"):
        (job_dir / sub).mkdir(exist_ok=True)
    return job_dir


def write_job_plan(job_dir: Path, plan: dict[str, Any]) -> None:
    """Write job.json (input plan)."""
    with open(job_dir / "job.json", "w", encoding="utf-8") as f:
        json.dump(plan, f, indent=2, ensure_ascii=False)


def write_result(job_dir: Path, result: dict[str, Any]) -> None:
    """Write result.json (output)."""
    with open(job_dir / "result.json", "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
