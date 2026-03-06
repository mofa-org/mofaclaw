"""
Background music: video + plan -> final MP4 with mixed audio.

Port logic from Clip-Forge background_music.py (yt-dlp + volume balance).
"""

from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from .jobs import JobLogger


def add_background(
    video_path: Path, plan: dict[str, Any], job_dir: Path, log: "JobLogger"
) -> Path:
    """
    Search theme, download, mix with voiceover, write final MP4.
    Returns path to final video.
    """
    raise NotImplementedError(
        "music.add_background: Port from Clip-Forge background_music.py. "
        "Requires: yt-dlp, pydub, FFmpeg"
    )
