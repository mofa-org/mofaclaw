"""
Image search and processing: tags -> portrait images.

Port logic from Clip-Forge image_downloader.py (Serper + crop + optional Waifu2x).
"""

from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from .jobs import JobLogger


def download_all(
    tags: list[str], plan: dict[str, Any], job_dir: Path, log: "JobLogger"
) -> list[Path]:
    """
    Download images per tag, crop to portrait, optional upscale.
    Returns list of image paths.
    """
    raise NotImplementedError(
        "images.download_all: Port from Clip-Forge image_downloader.py. "
        "Requires: requests, opencv-python-headless, Pillow, SERPER_API_KEY"
    )
