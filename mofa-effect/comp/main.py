"""
MoFA Effect v2 - Main Entry Point
DaVinci Resolve Free Edition (Workspace > Scripts approach)

Pipeline:
  1. Parse .comp       - detect text / image / color / text_color slots
  2. User edits        - type new text, supply your image path, pick colors
  3. Auto-placeholders - generate placeholder media for missing files
  4. Patch .comp       - write modified copy to output/
  5. Font selection     - pick a safe font for all text
  6. Hand off to Resolve
"""

import os
import sys
import time

PROJECT_ROOT = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, PROJECT_ROOT)

from config import ensure_directories, OUTPUT_DIR, LOGS_DIR, IMAGES_DIR
from utils import Logger, save_json, ask_user
from comp_parser import parse_comp_file
from comp_modifier import apply_changes
from spline_evaluator import parse_all_splines
from font_picker import pick_font
from resolve_renderer import install_render_script
from placeholder_generator import create_placeholder_for_element


BANNER = """
================================================================
     MoFA Effect v2 - DaVinci Resolve Free Edition
     Parse -> Edit -> Patch -> Font Select -> Render
================================================================
"""


def get_comp_path(logger):
    logger.section("Select Template")
    print("\nDrag your .comp file here, or type its full path:")
    while True:
        path = ask_user("Comp file path").strip().strip('"').strip("'")
        if os.path.isfile(path):
            logger.info(f"Template: {path}")
            return path
        print("  File not found. Try again.")


def display_elements(elements):
    print("\n" + "=" * 65)
    print("  DETECTED ELEMENTS")
    print("=" * 65)

    text_count = 0
    image_count = 0
    color_count = 0
    text_color_count = 0

    for e in elements:
        idx = e["index"]
        name = e["tool_name"]

        if e["type"] == "text":
            text_count += 1
            vals = e.get("current_values", [e.get("current_value", "")])
            font = e.get("font", "")
            style = e.get("font_style", "")
            source = e.get("source", "inline")
            print(f"\n  {idx}. [TEXT]  \"{name}\"")
            print(f"     Current values : {vals}")
            if font:
                print(f"     Font           : {font} {style}")
            if source == "spline":
                print(f"     Type           : Animated (keyframed)")
                print(f"     Input format   : WORD1 / WORD2 / WORD3")
            else:
                print(f"     Type           : Static")

        elif e["type"] == "image":
            image_count += 1
            subtype = e.get("media_subtype", "image")
            current = e.get("current_value", "")
            current_exists = _file_exists(current)
            status = "" if current_exists else " [MISSING - will use placeholder]"
            print(f"\n  {idx}. [{subtype.upper()}] \"{name}\"")
            print(f"     Current file   : {current}{status}")

        elif e["type"] == "color":
            color_count += 1
            print(f"\n  {idx}. [COLOR] \"{name}\"")
            print(f"     Current color  : {e['current_value']}")
            print(f"     Input format   : #rrggbb")

        elif e["type"] == "text_color":
            text_color_count += 1
            print(f"\n  {idx}. [TEXT COLOR] \"{name}\"")
            print(f"     Current color  : {e['current_value']}")
            print(f"     Input format   : #rrggbb")

    print(f"\n  Summary: {text_count} text, {image_count} image/video, "
          f"{color_count} background color, {text_color_count} text color")
    print("=" * 65)


def interactive_edit(elements, template_data):
    n = len(elements)
    changed = 0
    width = template_data.get("width", 1920)
    height = template_data.get("height", 1080)
    duration_frames = template_data.get("duration_frames", 150)
    fps = template_data.get("fps", 30)

    print(f"\nEditing {n} element(s). Press Enter to keep current.\n")

    for e in elements:
        idx = e["index"]
        name = e["tool_name"]
        print(f"-- {idx}/{n} --")

        if e["type"] == "text":
            vals = e.get("current_values", [e.get("current_value", "")])
            font = e.get("font", "")
            source = e.get("source", "inline")
            print(f"[TEXT] \"{name}\"")
            if font:
                print(f"  Font: {font}")
            print(f"  Current: {vals}")
            if source == "spline":
                print("  New (e.g. WELCOME / TO / MOFA, or Enter to keep):")
                raw = input("> ").strip()
            else:
                print("  New text (Enter to keep):")
                raw = input("> ").strip()
            if raw:
                e["new_value"] = raw
                changed += 1
                if " / " in raw:
                    print(f"  -> {[v.strip() for v in raw.split('/')]}")
                else:
                    print(f"  -> \"{raw}\"")
            else:
                print("  -> kept")

        elif e["type"] == "image":
            subtype = e.get("media_subtype", "image")
            current = e.get("current_value", "")
            current_exists = _file_exists(current)

            print(f"[{subtype.upper()}] \"{name}\"")
            print(f"  Current: {current}")

            if not current_exists:
                print(f"  NOTE: Original file not found on your machine.")
                print(f"  If you skip, a placeholder will be auto-generated.")

            print("  Your file path (or type 'None' for placeholder):")
            raw = input("> ").strip().strip('"').strip("'")
            if raw:
                lower = raw.lower()
                if lower in ("none", "placeholder", "skip"):
                    # Explicit request for placeholder, regardless of whether current file exists
                    placeholder = create_placeholder_for_element(
                        e, width, height, duration_frames, fps
                    )
                    if placeholder:
                        e["new_value"] = placeholder
                        changed += 1
                        print(f"  -> Placeholder: {os.path.basename(placeholder)}")
                    else:
                        print("  -> Could not create placeholder. Element may fail.")
                elif os.path.isfile(raw):
                    e["new_value"] = os.path.abspath(raw)
                    changed += 1
                    print(f"  -> {os.path.basename(raw)}")
                else:
                    print(f"  -> File not found: {raw}")
                    print(f"  -> Will generate placeholder instead.")
                    placeholder = create_placeholder_for_element(
                        e, width, height, duration_frames, fps
                    )
                    if placeholder:
                        e["new_value"] = placeholder
                        changed += 1
                        print(f"  -> Placeholder: {os.path.basename(placeholder)}")
            else:
                if not current_exists:
                    placeholder = create_placeholder_for_element(
                        e, width, height, duration_frames, fps
                    )
                    if placeholder:
                        e["new_value"] = placeholder
                        changed += 1
                        print(f"  -> Placeholder: {os.path.basename(placeholder)}")
                    else:
                        print(f"  -> Could not create placeholder. Element may fail.")
                else:
                    print("  -> kept (original file exists)")

        elif e["type"] in ("color", "text_color"):
            label = "TEXT COLOR" if e["type"] == "text_color" else "COLOR"
            print(f"[{label}] \"{name}\"")
            print(f"  Current: {e['current_value']}")
            print("  New hex color, e.g. #ff3300 (Enter to keep):")
            raw = input("> ").strip()
            if raw:
                if not raw.startswith("#"):
                    raw = "#" + raw
                if len(raw) == 7:
                    try:
                        int(raw[1:], 16)
                        e["new_value"] = raw
                        changed += 1
                        print(f"  -> {raw}")
                    except ValueError:
                        print("  -> Invalid hex. Keeping.")
                else:
                    print("  -> Bad format (need 6 hex digits). Keeping.")
            else:
                print("  -> kept")

        print()

    return changed


def _file_exists(path):
    if not path:
        return False
    if path in ("(empty slot)", "(Resolve timeline media)"):
        return False
    return os.path.isfile(path)


def run(comp_path, logger):
    t0 = time.time()
    ensure_directories()

    # -- Phase 1: Parse --
    logger.section("PHASE 1: Parse .comp")
    data = parse_comp_file(comp_path, logger)
    if not data["valid"]:
        logger.error("Not a valid .comp file.")
        return False

    print(f"\n  Duration  : {data['duration_seconds']}s  "
          f"({data['duration_frames']} frames @ {data['fps']} fps)")
    print(f"  Resolution: {data['width']}x{data['height']}")
    print(f"  Elements  : {len(data['changeable_elements'])}")

    elements = data["changeable_elements"]
    if not elements:
        logger.warning("No changeable elements detected.")
        print("\n  No changeable elements found in this template.")
        return True

    # -- Phase 2: Parse splines (for font detection) --
    logger.section("PHASE 2: Parse Splines")
    splines = parse_all_splines(data["raw_content"])
    logger.info(f"Splines found: {len(splines)}")

    # -- Phase 3: User edits --
    logger.section("PHASE 3: Interactive Editing")
    display_elements(elements)
    do_edit = ask_user("Make changes? (yes/no)", "yes")
    n_changed = 0
    if do_edit.lower() in ("yes", "y"):
        n_changed = interactive_edit(elements, data)
        print(f"  {n_changed} element(s) changed.")
    else:
        # Even if user says no, auto-generate placeholders for missing media
        print("  No manual changes.")
        n_changed = _auto_placeholder_missing(elements, data, logger)
        if n_changed:
            print(f"  {n_changed} placeholder(s) auto-generated for missing media.")

    # -- Phase 4: Font selection --
    logger.section("PHASE 4: Font Selection")

    original_fonts = set(data.get("all_fonts_used", []))

    font_splines = [n for n in splines if n.endswith("Font")]
    for fsn in font_splines:
        sp = splines[fsn]
        for _, v in getattr(sp, "keyframes", []):
            if v:
                original_fonts.add(v)

    if original_fonts:
        print(f"\n  This .comp uses: {', '.join(sorted(original_fonts))}")
        print("  These fonts may not be installed. Pick a replacement.\n")
    else:
        print("\n  No specific fonts detected. Pick a font for your text.\n")

    replacement_font = pick_font("Choose the font to use for all text in this video")
    logger.info(f"Replacement font: {replacement_font}")

    # -- Phase 5: Patch the .comp --
    logger.section("PHASE 5: Patch .comp")
    data["replacement_font"] = replacement_font
    modified_comp = apply_changes(comp_path, data, font_fallback_map={}, logger=logger)
    logger.info(f"Modified .comp: {modified_comp}")

    # -- Phase 6: Hand off to Resolve --
    logger.section("PHASE 6: Hand Off to Resolve")

    import datetime
    ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    basename = os.path.splitext(os.path.basename(comp_path))[0]
    basename = basename.replace(" ", "_")
    output_mp4 = os.path.join(OUTPUT_DIR, f"{basename}_mofa_{ts}.mp4")

    install_render_script(modified_comp, output_mp4, PROJECT_ROOT, logger, comp_data=data)

    elapsed = time.time() - t0

    print(f"\n  Setup complete in {elapsed:.1f}s.")
    print("  Waiting for you to click the script in Resolve...")

    save_json({
        "comp": comp_path,
        "modified_comp": modified_comp,
        "expected_output": output_mp4,
        "replacement_font": replacement_font,
        "original_fonts": sorted(original_fonts),
        "elements_total": len(elements),
        "elements_changed": n_changed,
        "setup_elapsed_s": round(elapsed, 1),
    }, os.path.join(OUTPUT_DIR, "render_report.json"))

    return True


def _auto_placeholder_missing(elements, template_data, logger):
    """Auto-generate placeholders for any image/video elements whose files are missing."""
    count = 0
    width = template_data.get("width", 1920)
    height = template_data.get("height", 1080)
    duration_frames = template_data.get("duration_frames", 150)
    fps = template_data.get("fps", 30)

    for e in elements:
        if e["type"] != "image":
            continue
        if e.get("new_value"):
            continue

        current = e.get("current_value", "")
        if _file_exists(current):
            continue

        placeholder = create_placeholder_for_element(
            e, width, height, duration_frames, fps
        )
        if placeholder:
            e["new_value"] = placeholder
            count += 1
            logger.info(f"  Auto-placeholder for [{e['tool_name']}]: {os.path.basename(placeholder)}")

    return count


def main():
    print(BANNER)
    ensure_directories()
    logger = Logger(LOGS_DIR)
    logger.info(f"Python {sys.version}")
    logger.info(f"Project root: {PROJECT_ROOT}")

    try:
        comp_path = get_comp_path(logger)
        run(comp_path, logger)
    except KeyboardInterrupt:
        print("\nCancelled.")
    except Exception as exc:
        import traceback
        logger.error(f"Unhandled error: {exc}")
        logger.error(traceback.format_exc())
        print(f"\nError: {exc}")
        print(f"Check log: {logger.log_file}")


if __name__ == "__main__":
    main()