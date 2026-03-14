"""
MoFA Effect v2 - Utilities
"""

import os
import json
import datetime


class Logger:
    def __init__(self, log_dir):
        os.makedirs(log_dir, exist_ok=True)
        ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
        self.log_file = os.path.join(log_dir, f"mofa_run_{ts}.log")

    def log(self, level, msg):
        ts = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        entry = f"[{ts}] [{level}] {msg}"
        print(entry)
        try:
            with open(self.log_file, "a", encoding="utf-8") as f:
                f.write(entry + "\n")
        except Exception:
            pass

    def info(self, msg):
        self.log("INFO", msg)

    def error(self, msg):
        self.log("ERROR", msg)

    def warning(self, msg):
        self.log("WARNING", msg)

    def section(self, title):
        self.log("INFO", "=" * 60)
        self.log("INFO", f"  {title}")
        self.log("INFO", "=" * 60)


def save_json(data, filepath):
    os.makedirs(os.path.dirname(filepath), exist_ok=True)
    with open(filepath, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)


def ask_user(prompt, default=None):
    """
    Ask the user a question in a way that works both in a real terminal
    and when stdout is being mirrored line-by-line (e.g. into Discord).

    We print the prompt as its own line, then read the answer on the next
    line so that log consumers can see every question clearly.
    """
    if default:
        print(f"{prompt} [{default}]:")
        val = input("> ").strip()
        return val if val else default

    print(f"{prompt}:")
    return input("> ").strip()