"""
Videolizer CLI: subcommands for full pipeline and individual modules.

Usage:
  python -m videolizer full --plan plan.json --out out_dir
  python -m videolizer voiceover --text "script" --out audio.wav
  python -m videolizer subtitles --audio voice.wav --text script.txt --out subs.srt
"""

import argparse
import json
import sys
import time
from pathlib import Path

from .contracts import VideolizeResult
from .jobs import JobLogger, create_job_dir, write_job_plan, write_result


def cmd_full(args: argparse.Namespace) -> int:
    """Run full video pipeline from VideoPlan."""
    plan_path = Path(args.plan)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    with open(plan_path, encoding="utf-8") as f:
        plan_data = json.load(f)

    job_id = plan_data.get("job_id", "unknown")
    job_dir = create_job_dir(out_dir.parent, job_id) if out_dir.name != job_id else out_dir
    write_job_plan(job_dir, plan_data)

    log = JobLogger(job_dir)
    log.info("Starting full videolizer pipeline", job_id=job_id)

    try:
        # Step 1: Generate script (content)
        log.step_start("content_generate")
        t0 = time.perf_counter()
        from . import content  # noqa: F401

        script = content.generate_script(plan_data["character"], plan_data["series"], log)
        log.step_end("content_generate", time.perf_counter() - t0)

        # Step 2: Voiceover
        log.step_start("voiceover")
        t0 = time.perf_counter()
        from . import voiceover  # noqa: F401

        voice_path = voiceover.generate(script, job_dir / "audio" / "voiceover.wav", log)
        log.step_end("voiceover", time.perf_counter() - t0)

        # Step 3: Smart sync + segments
        log.step_start("sync")
        t0 = time.perf_counter()
        from . import sync  # noqa: F401

        sync_result = sync.integrate(plan_data, script, str(voice_path), job_dir, log)
        log.step_end("sync", time.perf_counter() - t0)

        # Step 4: Images
        log.step_start("images")
        t0 = time.perf_counter()
        from . import images  # noqa: F401

        image_paths = images.download_all(sync_result["tags"], plan_data, job_dir, log)
        log.step_end("images", time.perf_counter() - t0)

        # Step 5: Subtitles
        log.step_start("subtitles")
        t0 = time.perf_counter()
        from . import subtitles  # noqa: F401

        subs_path = subtitles.generate(script, voice_path, job_dir, log)
        log.step_end("subtitles", time.perf_counter() - t0)

        # Step 6: Video assembly
        log.step_start("video")
        t0 = time.perf_counter()
        from . import video  # noqa: F401

        video_path = video.assemble(
            image_paths, voice_path, subs_path, sync_result.get("timed_segments"), job_dir, log
        )
        log.step_end("video", time.perf_counter() - t0)

        # Step 7: Background music
        log.step_start("music")
        t0 = time.perf_counter()
        from . import music  # noqa: F401

        final_path = music.add_background(video_path, plan_data, job_dir, log)
        log.step_end("music", time.perf_counter() - t0)

        result = VideolizeResult(
            status="success",
            job_id=job_id,
            artifacts={
                "script": str(job_dir / "script.txt"),
                "voiceover": str(voice_path),
                "subtitles": str(subs_path),
                "final_mp4": str(final_path),
                "logs": str(job_dir / "logs.txt"),
            },
            metrics={"total_steps": 7},
        )
    except Exception as e:
        log.error(str(e), exc_info=True)
        result = VideolizeResult(
            status="failure",
            job_id=job_id,
            error=str(e),
            artifacts={"logs": str(job_dir / "logs.txt")},
        )

    log.close()
    write_result(job_dir, result.to_dict())
    print(json.dumps(result.to_dict(), indent=2))
    return 0 if result.status == "success" else 1


def cmd_voiceover(args: argparse.Namespace) -> int:
    """Generate voiceover only: text -> WAV/MP3."""
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    from . import voiceover  # noqa: F401

    log = JobLogger(out_path.parent)
    voiceover.generate(args.text, out_path, log)
    log.close()
    print(str(out_path))
    return 0


def cmd_subtitles(args: argparse.Namespace) -> int:
    """Generate subtitles only: audio + text -> SRT."""
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    from . import subtitles  # noqa: F401

    log = JobLogger(out_path.parent)
    subtitles.generate_from_files(args.text, args.audio, out_path, log)
    log.close()
    print(str(out_path))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(prog="videolizer")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # full
    p_full = subparsers.add_parser("full", help="Run full video pipeline")
    p_full.add_argument("--plan", required=True, help="Path to VideoPlan JSON")
    p_full.add_argument("--out", required=True, help="Output directory")
    p_full.set_defaults(func=cmd_full)

    # voiceover
    p_vo = subparsers.add_parser("voiceover", help="Generate voiceover from text only")
    p_vo.add_argument("--text", required=True, help="Script text")
    p_vo.add_argument("--out", required=True, help="Output WAV/MP3 path")
    p_vo.set_defaults(func=cmd_voiceover)

    # subtitles
    p_subs = subparsers.add_parser("subtitles", help="Generate subtitles from audio + text")
    p_subs.add_argument("--text", required=True, help="Script text")
    p_subs.add_argument("--audio", required=True, help="Audio file path")
    p_subs.add_argument("--out", required=True, help="Output SRT path")
    p_subs.set_defaults(func=cmd_subtitles)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
