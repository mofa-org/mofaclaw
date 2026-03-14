"""
MoFA Effect v2 - Universal Fusion .comp Modifier

Applies user changes to any .comp file format:
  - Inline text (plain and bracket formats)
  - Spline text keyframes
  - Image paths (Clips block and Input format)
  - Background colors (inline and spline-driven)
  - Text colors (inline Red1/Green1/Blue1)
  - Font and Style replacement (spline and inline, both formats)
"""

import os
import re
import shutil
import time

from config import OUTPUT_DIR


def apply_changes(original_path, template_data, font_fallback_map, logger):
    logger.section("Applying Changes to .comp File")

    basename = os.path.splitext(os.path.basename(original_path))[0]
    basename = basename.replace(" ", "_")
    timestamp = int(time.time())
    modified_path = os.path.join(OUTPUT_DIR, f"{basename}_mofa_{timestamp}.comp")

    shutil.copy2(original_path, modified_path)
    logger.info(f"Working copy: {modified_path}")

    content = _read_file(modified_path)
    changes = 0
    elements = template_data.get("changeable_elements", [])

    for elem in elements:
        if elem.get("new_value") is None:
            continue

        if elem["type"] == "text":
            source = elem.get("source", "inline")

            if source == "spline":
                spline_name = elem.get("spline_name")
                new_vals = elem.get("new_values") or _expand_new_values(
                    elem["new_value"], elem.get("current_values", [])
                )
                content, ok = _replace_spline_text(content, spline_name, new_vals)
                if ok:
                    logger.info(f"  Text spline [{spline_name}] -> {new_vals}")
                    changes += 1
            else:
                content, ok = _replace_inline_text(content, elem["tool_name"], elem["new_value"])
                if ok:
                    logger.info(f"  Text [{elem['tool_name']}] -> \"{elem['new_value']}\"")
                    changes += 1

        elif elem["type"] == "image":
            new_path = elem["new_value"]
            if os.path.isfile(new_path):
                norm_path = new_path.replace("\\", "/")
                content, ok = _replace_loader_filename(content, elem["tool_name"], norm_path)
                if ok:
                    logger.info(f"  Image [{elem['tool_name']}] -> {os.path.basename(new_path)}")
                    changes += 1

        elif elem["type"] == "color":
            content, ok = _replace_background_color(content, elem["tool_name"], elem["new_value"])
            if ok:
                logger.info(f"  Color [{elem['tool_name']}] -> {elem['new_value']}")
                changes += 1

        elif elem["type"] == "text_color":
            content, ok = _replace_text_color(content, elem["tool_name"], elem["new_value"])
            if ok:
                logger.info(f"  Text color [{elem['tool_name']}] -> {elem['new_value']}")
                changes += 1

    # Legacy font fallback map
    for original_font, fallback_font in font_fallback_map.items():
        content, ok = _replace_font_globally(content, original_font, fallback_font)
        if ok:
            logger.info(f"  Font \"{original_font}\" -> \"{fallback_font}\" (fallback)")
            changes += 1

    # Nuclear font rewrite if a replacement font was chosen
    replacement_font = template_data.get("replacement_font")
    if replacement_font:
        content, fc = _rewrite_all_fonts(content, replacement_font, logger)
        changes += fc

    with open(modified_path, "w", encoding="utf-8") as f:
        f.write(content)

    logger.info(f"Total changes applied: {changes}")
    logger.info(f"Modified file saved: {modified_path}")
    return modified_path


# ======================================================================
# Helpers
# ======================================================================

def _read_file(path):
    for enc in ["utf-8-sig", "utf-8", "latin-1"]:
        try:
            with open(path, "r", encoding=enc) as f:
                return f.read()
        except UnicodeDecodeError:
            continue
    return ""


def _find_block_end(content, start_pos):
    depth = 1
    i = start_pos
    n = len(content)
    while i < n and depth > 0:
        if content[i] == '{':
            depth += 1
        elif content[i] == '}':
            depth -= 1
        i += 1
    return i


def _expand_new_values(new_value_str, current_values):
    parts = [p.strip() for p in new_value_str.split(" / ") if p.strip()]
    if not parts:
        parts = [new_value_str]
    n = max(len(current_values), 1)
    if len(parts) < n:
        parts = parts + [parts[-1]] * (n - len(parts))
    return parts[:n] if len(parts) > n else parts


def _escape_lua(text):
    return text.replace("\\", "\\\\").replace('"', '\\"')


# ======================================================================
# Text Replacement
# ======================================================================

def _replace_spline_text(content, spline_name, new_values):
    pattern = re.compile(
        rf'{re.escape(spline_name)}\s*=\s*BezierSpline\s*\{{', re.MULTILINE
    )
    m = pattern.search(content)
    if not m:
        return content, False

    block_start = m.end()
    block_end = _find_block_end(content, block_start)
    block = content[block_start:block_end]

    kf_pattern = re.compile(r'(Value\s*=\s*Text\s*\{\s*Value\s*=\s*)"([^"]*)"')
    idx = [0]

    def replacer(mo):
        i = idx[0]
        idx[0] += 1
        val = new_values[min(i, len(new_values) - 1)]
        return mo.group(1) + f'"{_escape_lua(val)}"'

    new_block, count = kf_pattern.subn(replacer, block)
    if count == 0:
        return content, False

    content = content[:block_start] + new_block + content[block_end:]
    return content, True


def _replace_inline_text(content, tool_name, new_text):
    """Replace StyledText in a TextPlus/Text3D tool. Handles all formats."""
    escaped_name = re.escape(tool_name)
    tool_match = re.search(
        rf'{escaped_name}\s*=\s*(\w+)\s*\{{', content
    )
    if not tool_match:
        return content, False

    start = tool_match.end()
    block_end = _find_block_end(content, start)
    region = content[start:block_end]

    safe_text = _escape_lua(new_text)

    # Try all known StyledText patterns
    patterns = [
        re.compile(r'(\bStyledText\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
        re.compile(r'(\["StyledText"\]\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
    ]

    for pat in patterns:
        mo = pat.search(region)
        if mo:
            abs_start = start + mo.start(2)
            abs_end = start + mo.end(2)
            content = content[:abs_start] + safe_text + content[abs_end:]
            return content, True

    return content, False


# ======================================================================
# Image Replacement
# ======================================================================

def _replace_loader_filename(content, tool_name, new_path):
    escaped_name = re.escape(tool_name)
    tool_m = re.search(rf'{escaped_name}\s*=\s*Loader\s*\{{', content)
    if not tool_m:
        return content, False

    block_start = tool_m.end()
    block_end = _find_block_end(content, block_start)
    block = content[block_start:block_end]
    changed = False

    # Method 1: Clips block
    clips_m = re.search(r'\bClips\s*=\s*\{', block)
    if clips_m:
        clips_start = clips_m.end()
        clips_end_pos = _find_block_end(block, clips_start) - block_start
        clips_inner = block[clips_m.end():clips_end_pos + block_start]

        fn_pat = re.compile(r'(Filename\s*=\s*)"([^"]*)"')
        fn_mo = fn_pat.search(clips_inner)
        if fn_mo:
            # Replace within the clips inner region
            rep_start = clips_m.end() + fn_mo.start(2)
            rep_end = clips_m.end() + fn_mo.end(2)
            block = block[:rep_start] + new_path + block[rep_end:]
            changed = True

    # Method 2: Direct Filename Input
    if not changed:
        for pat in [
            re.compile(r'(\bFilename\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
            re.compile(r'(\["Filename"\]\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
        ]:
            mo = pat.search(block)
            if mo:
                block = block[:mo.start(2)] + new_path + block[mo.end(2):]
                changed = True
                break

    # Update MEDIA_PATH and MEDIA_NAME in CustomData
    if changed:
        mp_pat = re.compile(r'(MEDIA_PATH\s*=\s*)"([^"]*)"')
        mp_mo = mp_pat.search(block)
        if mp_mo:
            block = block[:mp_mo.start(2)] + new_path + block[mp_mo.end(2):]

        mn_pat = re.compile(r'(MEDIA_NAME\s*=\s*)"([^"]*)"')
        mn_mo = mn_pat.search(block)
        if mn_mo:
            block = block[:mn_mo.start(2)] + os.path.basename(new_path) + block[mn_mo.end(2):]

    if changed:
        content = content[:block_start] + block + content[block_end:]

    return content, changed


# ======================================================================
# Background Color Replacement
# ======================================================================

def _replace_background_color(content, tool_name, hex_color):
    hex_color = hex_color.lstrip("#")
    if len(hex_color) != 6:
        return content, False
    try:
        r = int(hex_color[0:2], 16) / 255.0
        g = int(hex_color[2:4], 16) / 255.0
        b = int(hex_color[4:6], 16) / 255.0
    except ValueError:
        return content, False

    escaped_name = re.escape(tool_name)
    tool_m = re.search(rf'{escaped_name}\s*=\s*Background\s*\{{', content)
    if not tool_m:
        return content, False

    block_start = tool_m.end()
    block_end = _find_block_end(content, block_start)
    block = content[block_start:block_end]
    changed = False

    for key, val in [("TopLeftRed", r), ("TopLeftGreen", g), ("TopLeftBlue", b)]:
        # Try inline replacement (both formats)
        replaced = False
        for pat in [
            re.compile(rf'(\b{key}\s*=\s*Input\s*\{{\s*Value\s*=\s*)[\d.]+'),
            re.compile(rf'(\["{key}"\]\s*=\s*Input\s*\{{\s*Value\s*=\s*)[\d.]+'),
        ]:
            mo = pat.search(block)
            if mo:
                block = block[:mo.end(1)] + f"{val:.6f}" + block[mo.end():]
                changed = True
                replaced = True
                break

        # If not inline, try SourceOp spline
        if not replaced:
            for src_pat in [
                re.compile(rf'\b{key}\s*=\s*Input\s*\{{\s*SourceOp\s*=\s*"(\w+)"'),
                re.compile(rf'\["{key}"\]\s*=\s*Input\s*\{{\s*SourceOp\s*=\s*"(\w+)"'),
            ]:
                src_mo = src_pat.search(block)
                if src_mo:
                    spline_name = src_mo.group(1)
                    content, ok = _replace_all_spline_values(content, spline_name, val)
                    if ok:
                        changed = True
                    break

    # Only rewrite block if we changed inline values (spline changes are global)
    has_inline = any(
        re.search(rf'(\b{k}|\["{k}"\])\s*=\s*Input\s*\{{\s*Value', block)
        for k in ("TopLeftRed", "TopLeftGreen", "TopLeftBlue")
    )
    if changed and has_inline:
        content = content[:block_start] + block + content[block_end:]

    return content, changed


def _replace_all_spline_values(content, spline_name, new_val):
    pattern = re.compile(
        rf'{re.escape(spline_name)}\s*=\s*BezierSpline\s*\{{', re.MULTILINE
    )
    m = pattern.search(content)
    if not m:
        return content, False

    block_start = m.end()
    block_end = _find_block_end(content, block_start)
    block = content[block_start:block_end]

    kf_pat = re.compile(r'(\[\s*[\d.]+\s*\]\s*=\s*\{)\s*([\d.]+)')
    new_block = kf_pat.sub(lambda mo: mo.group(1) + f" {new_val:.6f}", block)

    if new_block == block:
        return content, False

    content = content[:block_start] + new_block + content[block_end:]
    return content, True


# ======================================================================
# Text Color Replacement
# ======================================================================

def _replace_text_color(content, tool_name, hex_color):
    """Replace Red1/Green1/Blue1 in a TextPlus/Text3D tool."""
    hex_color = hex_color.lstrip("#")
    if len(hex_color) != 6:
        return content, False
    try:
        r = int(hex_color[0:2], 16) / 255.0
        g = int(hex_color[2:4], 16) / 255.0
        b = int(hex_color[4:6], 16) / 255.0
    except ValueError:
        return content, False

    escaped_name = re.escape(tool_name)
    tool_m = re.search(rf'{escaped_name}\s*=\s*\w+\s*\{{', content)
    if not tool_m:
        return content, False

    block_start = tool_m.end()
    block_end = _find_block_end(content, block_start)
    block = content[block_start:block_end]
    changed = False

    for key, val in [("Red1", r), ("Green1", g), ("Blue1", b)]:
        for pat in [
            re.compile(rf'(\b{key}\s*=\s*Input\s*\{{\s*Value\s*=\s*)[\d.eE+-]+'),
            re.compile(rf'(\["{key}"\]\s*=\s*Input\s*\{{\s*Value\s*=\s*)[\d.eE+-]+'),
        ]:
            mo = pat.search(block)
            if mo:
                block = block[:mo.end(1)] + f"{val:.15f}" + block[mo.end():]
                changed = True
                break

    if changed:
        content = content[:block_start] + block + content[block_end:]
    return content, changed


# ======================================================================
# Font Replacement
# ======================================================================

def _replace_font_globally(content, original_font, fallback_font):
    count = 0
    for pat in [
        re.compile(rf'(\bFont\s*=\s*Input\s*\{{\s*Value\s*=\s*"){re.escape(original_font)}"'),
        re.compile(rf'(\["Font"\]\s*=\s*Input\s*\{{\s*Value\s*=\s*"){re.escape(original_font)}"'),
        re.compile(rf'(Value\s*=\s*Text\s*\{{\s*Value\s*=\s*"){re.escape(original_font)}"'),
    ]:
        content, c = pat.subn(rf'\g<1>{fallback_font}"', content)
        count += c
    return content, count > 0


def _rewrite_all_fonts(content, replacement_font, logger):
    """
    Nuclear font rewrite: replace ALL font names and styles everywhere.
    Handles spline keyframes AND inline inputs in both formats.
    """
    changes = 0

    # 1. Rewrite *Font spline keyframes
    font_text_pat = re.compile(
        r'(Value\s*=\s*Text\s*\{\s*\n\s*Value\s*=\s*")([^"]+)(")'
    )
    font_spline_pat = re.compile(r'(\w+Font)\s*=\s*BezierSpline\s*\{', re.MULTILINE)
    for m in list(font_spline_pat.finditer(content))[::-1]:
        spline_name = m.group(1)
        block_start = m.end()
        block_end = _find_block_end(content, block_start)
        block = content[block_start:block_end]
        new_block = font_text_pat.sub(
            lambda mo: mo.group(1) + replacement_font + mo.group(3), block
        )
        if new_block != block:
            content = content[:block_start] + new_block + content[block_end:]
            changes += 1
            logger.info(f"  Font spline [{spline_name}] -> \"{replacement_font}\"")

    # 2. Rewrite *Style spline keyframes
    style_spline_pat = re.compile(r'(\w+Style)\s*=\s*BezierSpline\s*\{', re.MULTILINE)
    for m in list(style_spline_pat.finditer(content))[::-1]:
        spline_name = m.group(1)
        block_start = m.end()
        block_end = _find_block_end(content, block_start)
        block = content[block_start:block_end]
        new_block = font_text_pat.sub(
            lambda mo: mo.group(1) + "Regular" + mo.group(3), block
        )
        if new_block != block:
            content = content[:block_start] + new_block + content[block_end:]
            changes += 1
            logger.info(f"  Style spline [{spline_name}] -> \"Regular\"")

    # 3. Rewrite inline Font = Input { Value = "..." } (both formats)
    for pat in [
        re.compile(r'(\bFont\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
        re.compile(r'(\["Font"\]\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
    ]:
        new_content, c = pat.subn(rf'\g<1>"{replacement_font}"', content)
        if c > 0:
            content = new_content
            changes += c
            logger.info(f"  Inline Font inputs -> \"{replacement_font}\" ({c} replacements)")

    # 4. Rewrite inline Style = Input { Value = "..." } (both formats)
    for pat in [
        re.compile(r'(\bStyle\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
        re.compile(r'(\["Style"\]\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"'),
    ]:
        new_content, c = pat.subn(r'\g<1>"Regular"', content)
        if c > 0:
            content = new_content
            changes += c
            logger.info(f"  Inline Style inputs -> \"Regular\" ({c} replacements)")

    return content, changes