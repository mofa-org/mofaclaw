"""
MoFA Effect v2 - Placeholder Generator

Creates placeholder images or videos for missing media elements.
This ensures the render never fails due to missing source files.

For images: creates a solid color PNG with "PLACEHOLDER" text.
For videos: creates a short MP4 with colored frames using ffmpeg.
"""

import os
import subprocess

from config import IMAGES_DIR


def create_placeholder_for_element(element, width, height, duration_frames, fps):
    """
    Create a placeholder file appropriate for the element type.
    Returns the absolute path to the created file, or None on failure.
    """
    os.makedirs(IMAGES_DIR, exist_ok=True)

    tool_name = element.get("tool_name", "placeholder")
    subtype = element.get("media_subtype", "image")
    safe_name = tool_name.replace(" ", "_")

    if subtype == "video":
        return _create_placeholder_video(safe_name, width, height, duration_frames, fps)
    else:
        return _create_placeholder_image(safe_name, width, height)


def _create_placeholder_image(name, width, height):
    """Create a simple placeholder PNG using PIL."""
    output_path = os.path.join(IMAGES_DIR, f"placeholder_{name}.png")

    if os.path.isfile(output_path):
        return output_path

    try:
        from PIL import Image, ImageDraw, ImageFont

        bg_color = (30, 30, 40)
        accent_color = (80, 130, 220)
        text_color = (200, 200, 210)

        img = Image.new("RGB", (width, height), bg_color)
        draw = ImageDraw.Draw(img)

        # Draw border
        border = max(4, min(width, height) // 100)
        draw.rectangle(
            [(border, border), (width - border - 1, height - border - 1)],
            outline=accent_color, width=border
        )

        # Draw crosshatch pattern
        line_color = (40, 40, 55)
        step = max(width, height) // 10
        for offset in range(0, max(width, height) * 2, step):
            draw.line([(offset, 0), (0, offset)], fill=line_color, width=1)
            draw.line([(width - offset, 0), (width, offset)], fill=line_color, width=1)

        # Load font
        font_size = max(24, min(width, height) // 12)
        small_size = max(14, min(width, height) // 20)
        font = _load_font(font_size)
        small_font = _load_font(small_size)

        # Draw "PLACEHOLDER" text
        _draw_centered(draw, "PLACEHOLDER", font, text_color, width, height // 2 - font_size)
        _draw_centered(draw, f"{width}x{height}", small_font, accent_color, width, height // 2 + small_size)
        _draw_centered(draw, f"[{name}]", small_font, (120, 120, 130), width, height // 2 + small_size * 3)

        img.save(output_path, "PNG")
        return output_path

    except ImportError:
        # PIL not available, create a minimal valid PNG manually
        return _create_minimal_png(output_path, width, height)
    except Exception:
        return _create_minimal_png(output_path, width, height)


def _create_placeholder_video(name, width, height, duration_frames, fps):
    """
    Create a placeholder video using ffmpeg.
    Falls back to a still image if ffmpeg is not available.
    """
    output_path = os.path.join(IMAGES_DIR, f"placeholder_{name}.mp4")

    if os.path.isfile(output_path):
        return output_path

    duration_seconds = duration_frames / max(fps, 1)

    ffmpeg_path = _find_ffmpeg()
    if not ffmpeg_path:
        # No ffmpeg, fall back to a still image
        # Resolve can use a still image in a Loader even if the original was video
        return _create_placeholder_image(name, width, height)

    # Generate a video with a solid color background using ffmpeg
    cmd = [
        ffmpeg_path,
        "-f", "lavfi",
        "-i", f"color=c=0x1e1e28:s={width}x{height}:d={duration_seconds:.2f}:r={fps}",
        "-vf", (
            f"drawtext=text='PLACEHOLDER':fontsize={max(24, height // 12)}:"
            f"fontcolor=white:x=(w-text_w)/2:y=(h-text_h)/2-{height // 10},"
            f"drawtext=text='[{name}]':fontsize={max(16, height // 20)}:"
            f"fontcolor=gray:x=(w-text_w)/2:y=(h-text_h)/2+{height // 10}"
        ),
        "-c:v", "libx264",
        "-preset", "ultrafast",
        "-crf", "28",
        "-pix_fmt", "yuv420p",
        "-y",
        output_path
    ]

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if result.returncode == 0 and os.path.isfile(output_path) and os.path.getsize(output_path) > 1000:
            return output_path
    except Exception:
        pass

    # Fallback: try without drawtext filter (in case fontconfig is missing)
    cmd_simple = [
        ffmpeg_path,
        "-f", "lavfi",
        "-i", f"color=c=0x1e1e28:s={width}x{height}:d={duration_seconds:.2f}:r={fps}",
        "-c:v", "libx264",
        "-preset", "ultrafast",
        "-crf", "28",
        "-pix_fmt", "yuv420p",
        "-y",
        output_path
    ]

    try:
        result = subprocess.run(cmd_simple, capture_output=True, text=True, timeout=60)
        if result.returncode == 0 and os.path.isfile(output_path) and os.path.getsize(output_path) > 1000:
            return output_path
    except Exception:
        pass

    # Last resort: create a still image instead
    return _create_placeholder_image(name, width, height)


def _create_minimal_png(output_path, width, height):
    """
    Create a minimal valid PNG without any dependencies.
    Uses raw PNG construction with zlib compression.
    Creates a solid dark gray image.
    """
    import struct
    import zlib

    def _chunk(chunk_type, data):
        c = chunk_type + data
        crc = struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
        return struct.pack(">I", len(data)) + c + crc

    # Limit size for raw PNG generation (memory)
    w = min(width, 1920)
    h = min(height, 1920)

    # IHDR
    ihdr_data = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)  # 8-bit RGB

    # IDAT - create scanlines
    raw_data = b""
    row = bytes([30, 30, 40] * w)  # dark gray RGB
    for _ in range(h):
        raw_data += b"\x00" + row  # filter byte 0 + row data

    compressed = zlib.compress(raw_data)
    idat = _chunk(b"IDAT", compressed)

    # Assemble PNG
    png = b"\x89PNG\r\n\x1a\n"
    png += _chunk(b"IHDR", ihdr_data)
    png += idat
    png += _chunk(b"IEND", b"")

    with open(output_path, "wb") as f:
        f.write(png)

    return output_path


def _draw_centered(draw, text, font, color, canvas_width, y):
    bbox = draw.textbbox((0, 0), text, font=font)
    tw = bbox[2] - bbox[0]
    x = (canvas_width - tw) // 2
    draw.text((x + 2, y + 2), text, fill=(0, 0, 0), font=font)
    draw.text((x, y), text, fill=color, font=font)


def _load_font(size):
    try:
        from PIL import ImageFont
        for p in [
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\calibri.ttf",
        ]:
            try:
                return ImageFont.truetype(p, size)
            except Exception:
                continue
        return ImageFont.load_default()
    except ImportError:
        return None


def _find_ffmpeg():
    """Find ffmpeg binary."""
    try:
        import imageio_ffmpeg
        p = imageio_ffmpeg.get_ffmpeg_exe()
        if os.path.isfile(p):
            return p
    except Exception:
        pass

    try:
        result = subprocess.run(
            ["ffmpeg", "-version"], capture_output=True, text=True, timeout=10
        )
        if result.returncode == 0:
            return "ffmpeg"
    except Exception:
        pass

    return None