"""
Video assembly: images + audio + subtitles -> MP4.

Port logic from Clip-Forge video_creator.py (MoviePy + FFmpeg).
"""

from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

if TYPE_CHECKING:
    from .jobs import JobLogger


def assemble(
    image_paths: list[Path],
    audio_path: Path,
    subtitle_path: Path,
    timed_segments: Optional[list[dict[str, Any]]],
    job_dir: Path,
    log: "JobLogger",
) -> Path:
    """
    Assemble image clips with zoom, add audio, burn subtitles.
    Returns path to temp video (before music).
    """
    raise NotImplementedError(
        "video.assemble: Port from Clip-Forge video_creator.py. "
        "Requires: moviepy, FFmpeg"
    )
