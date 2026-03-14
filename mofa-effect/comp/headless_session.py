"""
Headless MoFA Effect session wrapper.

This module exposes a programmatic API for running a MoFA Effect session
without any interactive terminal I/O. It reuses the existing pipeline:

    parse_comp_file -> apply overrides -> auto-placeholders -> font selection
    -> apply_changes -> install_render_script

The intent is for external callers (e.g. a Discord bot) to supply all inputs
upfront and receive back the key output paths.
"""

from __future__ import annotations

import datetime
import os
from dataclasses import dataclass
from typing import Any, Dict, Mapping, Optional

from config import PROJECT_ROOT, OUTPUT_DIR, SAFE_FALLBACK_FONTS, ensure_directories
from utils import Logger, save_json
from comp_parser import parse_comp_file
from comp_modifier import apply_changes
from resolve_renderer import install_render_script
from main import _auto_placeholder_missing


@dataclass
class HeadlessSessionInputs:
    """
    Inputs required to run a headless MoFA Effect session.

    element_overrides:
      - key: element index (1-based, as assigned by parse_comp_file),
             either int or str form (e.g. 1 or \"1\").
      - value: override payload, typically a string:
          * text / spline text: new text (may contain \" / \" separators)
          * image/video: absolute file path to media on disk
          * color / text_color: hex string like \"#ff3300\"
    """

    comp_path: str
    element_overrides: Mapping[str | int, Any]
    font_name: Optional[str] = None
    auto_placeholder_missing: bool = True


@dataclass
class HeadlessSessionResult:
    modified_comp_path: str
    expected_output_mp4: str
    render_report_path: str
    template_data: Dict[str, Any]


def _normalize_overrides(raw: Mapping[str | int, Any]) -> Dict[int, Any]:
    """
    Normalize element_overrides keys to integer indices.
    """
    normalized: Dict[int, Any] = {}
    for key, value in raw.items():
        if isinstance(key, int):
            normalized[key] = value
            continue
        try:
            idx = int(str(key).strip())
        except (TypeError, ValueError):
            continue
        normalized[idx] = value
    return normalized


def _apply_element_overrides(
    elements: list[Dict[str, Any]], overrides: Mapping[int, Any], logger: Logger
) -> int:
    """
    Apply element_overrides to the parsed changeable_elements list.

    Returns the number of elements that received a new_value.
    """
    changed = 0
    for e in elements:
        idx = e.get("index")
        if not isinstance(idx, int):
            continue
        if idx not in overrides:
            continue

        override = overrides[idx]
        if override is None:
            continue

        # Simple strategy: we expect string-like overrides for all element types.
        if isinstance(override, str):
            override = override.strip()
            if not override:
                continue
            e["new_value"] = override
            changed += 1
            logger.info(f"Override applied to element #{idx} ({e.get('tool_name')}): {override}")
        else:
            # Fallback: assign raw value and log it.
            e["new_value"] = override
            changed += 1
            logger.info(
                f"Override applied to element #{idx} ({e.get('tool_name')}): "
                f"{repr(override)}"
            )

    return changed


def _choose_font_name(inputs: HeadlessSessionInputs) -> str:
    """
    Choose a font name for the session.
    """
    if inputs.font_name:
        return inputs.font_name

    if SAFE_FALLBACK_FONTS:
        return SAFE_FALLBACK_FONTS[0]

    return "Arial"


def run_headless_session(
    inputs: HeadlessSessionInputs,
    logger: Optional[Logger] = None,
) -> HeadlessSessionResult:
    """
    Run a non-interactive MoFA Effect session.

    This function:
      1) Ensures output directories exist.
      2) Parses the provided .comp file.
      3) Applies element overrides and (optionally) auto-generates placeholders.
      4) Chooses a font.
      5) Writes a modified .comp into OUTPUT_DIR.
      6) Installs the Resolve render script and writes the handoff JSON.
      7) Writes a render_report.json into OUTPUT_DIR.
    """
    ensure_directories()

    if logger is None:
        # Lazy-import LOGS_DIR to avoid circulars at module import time.
        from config import LOGS_DIR

        logger = Logger(LOGS_DIR)

    logger.section("HEADLESS MOFA SESSION")
    logger.info(f"Comp path: {inputs.comp_path}")

    # Phase 1: Parse .comp
    data = parse_comp_file(inputs.comp_path, logger)
    if not data.get("valid"):
        raise ValueError(f"Not a valid .comp file: {inputs.comp_path}")

    elements = data.get("changeable_elements", [])
    if not elements:
        logger.warning("No changeable elements detected in template.")

    # Phase 2: Apply explicit overrides (+ optional auto placeholders)
    overrides = _normalize_overrides(inputs.element_overrides)
    if overrides:
        n_overridden = _apply_element_overrides(elements, overrides, logger)
        logger.info(f"Element overrides applied: {n_overridden}")
    else:
        logger.info("No explicit element overrides provided.")

    data["changeable_elements"] = elements

    if inputs.auto_placeholder_missing:
        # Reuse the existing helper from main.py.
        auto_count = _auto_placeholder_missing(elements, data, logger)
        if auto_count:
            logger.info(f"Auto-generated {auto_count} placeholder media file(s).")

    # Phase 3: Font selection
    font_name = _choose_font_name(inputs)
    logger.info(f"Using font: {font_name}")
    data["replacement_font"] = font_name

    # Phase 4: Patch the .comp
    logger.section("APPLYING CHANGES TO .COMP")
    modified_comp = apply_changes(
        inputs.comp_path,
        data,
        font_fallback_map={},
        logger=logger,
    )
    logger.info(f"Modified .comp written to: {modified_comp}")

    # Phase 5: Hand off to Resolve (render script installation)
    logger.section("HANDOFF TO RESOLVE")
    ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    basename = os.path.splitext(os.path.basename(inputs.comp_path))[0]
    basename = basename.replace(" ", "_")
    expected_output = os.path.join(OUTPUT_DIR, f"{basename}_mofa_{ts}.mp4")

    install_render_script(
        modified_comp,
        expected_output,
        PROJECT_ROOT,
        logger,
        comp_data=data,
    )

    # Phase 6: Write render report metadata
    render_report_path = os.path.join(OUTPUT_DIR, "render_report.json")
    save_json(
        {
            "comp": inputs.comp_path,
            "modified_comp": modified_comp,
            "expected_output": expected_output,
            "replacement_font": font_name,
            "elements_total": len(elements),
            "setup_source": "headless_session",
        },
        render_report_path,
    )
    logger.info(f"Render report written: {render_report_path}")

    return HeadlessSessionResult(
        modified_comp_path=modified_comp,
        expected_output_mp4=expected_output,
        render_report_path=render_report_path,
        template_data=data,
    )


