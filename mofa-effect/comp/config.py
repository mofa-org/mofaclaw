"""
MoFA Effect v2 - Configuration
DaVinci Resolve / Fusion Edition
"""

import os

PROJECT_ROOT = os.path.dirname(os.path.abspath(__file__))
TEMPLATES_DIR = os.path.join(PROJECT_ROOT, "templates")
OUTPUT_DIR = os.path.join(PROJECT_ROOT, "output")
IMAGES_DIR = os.path.join(OUTPUT_DIR, "images")
AUDIO_DIR = os.path.join(OUTPUT_DIR, "audio")
LOGS_DIR = os.path.join(OUTPUT_DIR, "logs")
FRAMES_DIR = os.path.join(OUTPUT_DIR, "frames")

RESOLVE_DIR = r"E:\Softwares\Davinci"

WINDOWS_FONTS_DIR = r"C:\Windows\Fonts"

GROQ_MODEL = "llama-3.3-70b-versatile"
GROQ_API_BASE = "https://api.groq.com/openai/v1"

TTS_VOICE = "en-US-GuyNeural"
TTS_RATE = "+0%"

DEFAULT_FPS = 30
DEFAULT_WIDTH = 1920
DEFAULT_HEIGHT = 1080

SAFE_FALLBACK_FONTS = [
    "Arial",
    "Segoe UI",
    "Calibri",
    "Verdana",
    "Tahoma",
    "Times New Roman",
    "Courier New",
    "Consolas",
    "Georgia",
    "Trebuchet MS",
]


def ensure_directories():
    for d in [OUTPUT_DIR, IMAGES_DIR, AUDIO_DIR, LOGS_DIR, TEMPLATES_DIR, FRAMES_DIR]:
        os.makedirs(d, exist_ok=True)