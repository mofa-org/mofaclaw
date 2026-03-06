"""
Videolizer job contracts: VideoPlan (input) and VideolizeResult (output).

These define the stable boundary between MofaClaw's Remix Engine and the Videolizer.
"""

from dataclasses import dataclass, field
from typing import Any, Optional


@dataclass
class SegmentSpec:
    """One segment in the video timeline."""

    start_s: float
    duration_s: float
    narration_text: str
    visual_prompt_or_tags: str
    on_screen_text: Optional[str] = None


@dataclass
class AudioStyle:
    """TTS/voiceover style parameters."""

    voice_id: str = "default"
    pitch: float = 0.0
    speed: float = 0.75
    target_lufs: Optional[float] = None


@dataclass
class MusicStyle:
    """Background music parameters."""

    query: str
    mood: str = "calm"
    target_ratio: float = 0.12


@dataclass
class OutputSpec:
    """Output resolution and format."""

    width: int = 1080
    height: int = 1920
    fps: int = 24


@dataclass
class VideoPlan:
    """
    Input contract for the Videolizer.
    Produced by the Remix Engine; consumed by the Videolizer.
    """

    job_id: str
    character: str
    series: str
    seed: Optional[str] = None
    persona: Optional[dict[str, Any]] = None
    content_transform: Optional[str] = None
    format: str = "video_short"
    segments: list[SegmentSpec] = field(default_factory=list)
    audio_style: AudioStyle = field(default_factory=AudioStyle)
    subtitle_style: str = "glow"
    music_style: Optional[MusicStyle] = None
    output: OutputSpec = field(default_factory=OutputSpec)

    def to_dict(self) -> dict[str, Any]:
        """Serialize for JSON."""
        return {
            "job_id": self.job_id,
            "character": self.character,
            "series": self.series,
            "seed": self.seed,
            "persona": self.persona,
            "content_transform": self.content_transform,
            "format": self.format,
            "segments": [
                {
                    "start_s": s.start_s,
                    "duration_s": s.duration_s,
                    "narration_text": s.narration_text,
                    "visual_prompt_or_tags": s.visual_prompt_or_tags,
                    "on_screen_text": s.on_screen_text,
                }
                for s in self.segments
            ],
            "audio_style": {
                "voice_id": self.audio_style.voice_id,
                "pitch": self.audio_style.pitch,
                "speed": self.audio_style.speed,
                "target_lufs": self.audio_style.target_lufs,
            },
            "subtitle_style": self.subtitle_style,
            "music_style": (
                {
                    "query": self.music_style.query,
                    "mood": self.music_style.mood,
                    "target_ratio": self.music_style.target_ratio,
                }
                if self.music_style
                else None
            ),
            "output": {
                "width": self.output.width,
                "height": self.output.height,
                "fps": self.output.fps,
            },
        }


@dataclass
class VideolizeResult:
    """
    Output contract from the Videolizer.
    Returned to MofaClaw; includes artifact paths and metrics.
    """

    status: str  # "success" | "failure"
    job_id: str
    error: Optional[str] = None
    artifacts: dict[str, str] = field(default_factory=dict)
    timings: dict[str, float] = field(default_factory=dict)
    metrics: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        """Serialize for JSON."""
        return {
            "status": self.status,
            "job_id": self.job_id,
            "error": self.error,
            "artifacts": self.artifacts,
            "timings": self.timings,
            "metrics": self.metrics,
        }
