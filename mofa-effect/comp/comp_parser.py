"""
MoFA Effect v2 - Universal Fusion .comp Parser

Handles every known .comp input format:
  Plain:    StyledText = Input { Value = "hello", },
  Bracket:  ["StyledText"] = Input { Value = "hello" },
  SourceOp: StyledText = Input { SourceOp = "Text1StyledText", Source = "Value" },
  Spline:   Text1StyledText = BezierSpline { KeyFrames { ... Value = Text { Value = "hello" } } }
  Clips:    Clips { Clip { Filename = "/path/to/file.png" } }

Detects: Text content, fonts, text colors, image/video paths, background colors.
Works with all Fusion versions (standalone 5-9, Resolve 12-19+).
"""

import os
import re


# ======================================================================
# Constants
# ======================================================================

TEXT_TOOL_TYPES = {"TextPlus", "Text3D"}

SKIP_NAMES = {
    "Input", "Value", "Source", "SourceOp", "Clip", "Clips",
    "KeyFrames", "Flags", "RH", "LH", "CustomData", "Comments",
    "UserControls", "CtrlWZoom", "ViewInfo", "InstanceInput",
    "NamedInput", "Array", "Output", "Tools", "Transitions",
}

SKIP_TYPES = {
    "BezierSpline", "FlowView", "MultiView", "SplineEditorView",
    "TimelineView", "OperatorInfo", "Polyline", "Point", "FuID",
    "Input", "ordered", "StyledText",
}


# ======================================================================
# Universal Input Helpers
# ======================================================================

def extract_block(content, start_pos):
    """Extract content inside { } starting just after the opening brace."""
    depth = 1
    i = start_pos
    n = len(content)
    while i < n and depth > 0:
        if content[i] == '{':
            depth += 1
        elif content[i] == '}':
            depth -= 1
        i += 1
    return content[start_pos:i - 1]


def find_inputs_block(tool_block):
    """Find and extract the Inputs = { ... } section from a tool block."""
    m = re.search(r'\bInputs\s*=\s*\{', tool_block)
    if not m:
        return tool_block  # fallback: search whole block
    return extract_block(tool_block, m.end())


def get_input_string(block, name):
    """
    Get string value of a named Input. Handles all formats:
      Name = Input { Value = "str" }
      ["Name"] = Input { Value = "str" }
    Returns string or None.
    """
    patterns = [
        re.compile(rf'\b{re.escape(name)}\s*=\s*Input\s*\{{\s*Value\s*=\s*"([^"]*)"'),
        re.compile(rf'\["{re.escape(name)}"\]\s*=\s*Input\s*\{{\s*Value\s*=\s*"([^"]*)"'),
    ]
    for pat in patterns:
        m = pat.search(block)
        if m:
            return m.group(1)
    return None


def get_input_number(block, name):
    """
    Get numeric value of a named Input. Handles all formats.
    Returns float or None.
    """
    patterns = [
        re.compile(rf'\b{re.escape(name)}\s*=\s*Input\s*\{{\s*Value\s*=\s*([\d.eE+-]+)'),
        re.compile(rf'\["{re.escape(name)}"\]\s*=\s*Input\s*\{{\s*Value\s*=\s*([\d.eE+-]+)'),
    ]
    for pat in patterns:
        m = pat.search(block)
        if m:
            try:
                return float(m.group(1))
            except ValueError:
                continue
    return None


def get_input_sourceop(block, name):
    """
    Get SourceOp name of a named Input (meaning it is animated via a spline).
    Returns spline name string or None.
    """
    patterns = [
        re.compile(rf'\b{re.escape(name)}\s*=\s*Input\s*\{{\s*SourceOp\s*=\s*"(\w+)"'),
        re.compile(rf'\["{re.escape(name)}"\]\s*=\s*Input\s*\{{\s*SourceOp\s*=\s*"(\w+)"'),
    ]
    for pat in patterns:
        m = pat.search(block)
        if m:
            return m.group(1)
    return None


def has_input(block, name):
    """Check if a named Input exists in the block at all."""
    patterns = [
        re.compile(rf'\b{re.escape(name)}\s*=\s*Input\s*\{{'),
        re.compile(rf'\["{re.escape(name)}"\]\s*=\s*Input\s*\{{'),
    ]
    return any(p.search(block) for p in patterns)


# ======================================================================
# Tool Discovery
# ======================================================================

def find_all_tools(content):
    """
    Find all tool definitions in the .comp file.
    Returns list of (name, type, block, position).
    """
    tools = []
    pattern = re.compile(r'(\w+)\s*=\s*(\w+)\s*\{', re.MULTILINE)

    for m in pattern.finditer(content):
        name = m.group(1)
        tool_type = m.group(2)

        if name in SKIP_NAMES or tool_type in SKIP_TYPES:
            continue

        block = extract_block(content, m.end())

        # A real tool has Inputs, ViewInfo, or NameSet
        if not any(kw in block for kw in ("Inputs", "ViewInfo", "NameSet")):
            continue

        tools.append((name, tool_type, block, m.start()))

    return tools


# ======================================================================
# Text Element Detection
# ======================================================================

def find_text_splines(content, logger):
    """
    Find all <ToolName>StyledText = BezierSpline { ... } blocks.
    These contain animated text across keyframes.
    """
    elements = []
    spline_pattern = re.compile(
        r'(\w+)StyledText\s*=\s*BezierSpline\s*\{', re.MULTILINE
    )

    for match in spline_pattern.finditer(content):
        base_name = match.group(1)
        spline_name = f"{base_name}StyledText"

        block = extract_block(content, match.end())

        text_values = re.findall(
            r'Value\s*=\s*Text\s*\{\s*Value\s*=\s*"([^"]*)"\s*\}',
            block
        )

        seen = set()
        unique_vals = []
        for v in text_values:
            if v and v not in seen:
                seen.add(v)
                unique_vals.append(v)

        display = unique_vals if unique_vals else ["(empty)"]

        logger.info(
            f"  Text spline [{spline_name}]: {unique_vals} "
            f"({len(text_values)} keyframes)"
        )

        elements.append({
            "type": "text",
            "tool_name": base_name,
            "spline_name": spline_name,
            "source": "spline",
            "current_values": unique_vals,
            "current_value": " / ".join(display),
            "new_value": None,
            "new_values": None,
            "keyframe_count": len(text_values),
            "position": match.start(),
        })

    return elements


def find_inline_text_tools(content, all_tools, spline_tool_names, logger):
    """
    Find TextPlus/Text3D tools with inline (non-animated) StyledText.
    Handles both plain and bracket input formats.
    """
    elements = []

    for name, tool_type, block, position in all_tools:
        if tool_type not in TEXT_TOOL_TYPES:
            continue
        if name in spline_tool_names:
            continue

        inputs = find_inputs_block(block)

        # Skip if StyledText is driven by a SourceOp (spline-animated)
        if get_input_sourceop(inputs, "StyledText"):
            continue

        # Get the text value
        text_val = get_input_string(inputs, "StyledText")
        if text_val is None:
            continue

        # Get font info
        font_val = get_input_string(inputs, "Font") or ""
        style_val = get_input_string(inputs, "Style") or ""
        size_val = get_input_number(inputs, "Size")

        # Get text color
        color = {}
        for channel, label in [("Red1", "r"), ("Green1", "g"), ("Blue1", "b")]:
            val = get_input_number(inputs, channel)
            if val is not None:
                color[label] = val

        logger.info(
            f"  Inline text [{name}]: \"{text_val}\" "
            f"(font: {font_val} {style_val})"
        )

        elements.append({
            "type": "text",
            "tool_name": name,
            "tool_type": tool_type,
            "spline_name": None,
            "source": "inline",
            "current_values": [text_val] if text_val else [],
            "current_value": text_val if text_val else "(empty)",
            "new_value": None,
            "new_values": None,
            "font": font_val,
            "font_style": style_val,
            "font_size": size_val if size_val else 0.05,
            "text_color": color,
            "position": position,
        })

    return elements


def find_text_color_elements(all_tools, logger):
    """
    Find text color (Red1/Green1/Blue1) in TextPlus/Text3D tools.
    Returns these as separate changeable color elements.
    """
    elements = []

    for name, tool_type, block, position in all_tools:
        if tool_type not in TEXT_TOOL_TYPES:
            continue

        inputs = find_inputs_block(block)

        r = get_input_number(inputs, "Red1")
        g = get_input_number(inputs, "Green1")
        b = get_input_number(inputs, "Blue1")

        # Only create element if at least one channel is explicitly set
        if r is None and g is None and b is None:
            continue

        r = r if r is not None else 0.0
        g = g if g is not None else 0.0
        b = b if b is not None else 0.0

        hex_color = rgb_float_to_hex(r, g, b)
        logger.info(f"  Text color [{name}]: {hex_color}")

        elements.append({
            "type": "text_color",
            "tool_name": name,
            "tool_type": tool_type,
            "current_value": hex_color,
            "current_rgb": {"r": r, "g": g, "b": b},
            "new_value": None,
            "position": position,
        })

    return elements


# ======================================================================
# Image / Video Detection
# ======================================================================

def find_loader_tools(content, all_tools, logger):
    """
    Find all Loader tools and extract their Filename.
    Handles all known filename storage patterns:
      1. Clips { Clip { Filename = "path" } }
      2. Filename = Input { Value = "path" }
      3. Filename = "path" (direct in Clips block)
      4. MEDIA_PATH in CustomData
    """
    elements = []

    for name, tool_type, block, position in all_tools:
        if tool_type != "Loader":
            continue

        filename = ""

        # Method 1: Filename inside Clips block
        clips_m = re.search(r'\bClips\s*=\s*\{', block)
        if clips_m:
            clips_block = extract_block(block, clips_m.end())
            fn_m = re.search(r'Filename\s*=\s*"([^"]*)"', clips_block)
            if fn_m:
                filename = fn_m.group(1)

        # Method 2: Filename as Input value
        if not filename:
            inputs = find_inputs_block(block)
            fn = get_input_string(inputs, "Filename")
            if fn:
                filename = fn

        # Method 3: MEDIA_PATH in CustomData
        if not filename:
            mp_m = re.search(r'MEDIA_PATH\s*=\s*"([^"]+)"', block)
            if mp_m:
                filename = mp_m.group(1)

        ext = os.path.splitext(filename)[1].lower() if filename else ""
        video_exts = {".mp4", ".mov", ".avi", ".mkv", ".webm", ".mxf", ".wmv", ".flv"}
        media_type = "video" if ext in video_exts else "image"

        logger.info(f"  {media_type.capitalize()} slot [{name}]: \"{filename}\"")

        elements.append({
            "type": "image",
            "tool_name": name,
            "tool_type": "Loader",
            "media_subtype": media_type,
            "current_value": filename if filename else "(empty slot)",
            "new_value": None,
            "position": position,
        })

    return elements


def find_mediain_tools(all_tools, logger):
    """Find MediaIn tools (Resolve timeline media references)."""
    elements = []
    for name, tool_type, block, position in all_tools:
        if tool_type != "MediaIn":
            continue
        logger.info(f"  Media slot [{name}]: (timeline reference)")
        elements.append({
            "type": "image",
            "tool_name": name,
            "tool_type": "MediaIn",
            "media_subtype": "timeline_media",
            "current_value": "(Resolve timeline media)",
            "new_value": None,
            "position": position,
        })
    return elements


# ======================================================================
# Background Color Detection
# ======================================================================

def find_background_tools(content, all_tools, logger):
    """
    Find all Background tools and extract their color.
    Colors can be inline or driven by BezierSplines.
    """
    elements = []

    for name, tool_type, block, position in all_tools:
        if tool_type != "Background":
            continue

        inputs = find_inputs_block(block)

        r_val = _get_color_channel(content, inputs, "TopLeftRed")
        g_val = _get_color_channel(content, inputs, "TopLeftGreen")
        b_val = _get_color_channel(content, inputs, "TopLeftBlue")

        hex_color = rgb_float_to_hex(r_val, g_val, b_val)
        logger.info(f"  Color slot [{name}]: {hex_color}")

        elements.append({
            "type": "color",
            "tool_name": name,
            "tool_type": "Background",
            "current_value": hex_color,
            "current_rgb": {"r": r_val, "g": g_val, "b": b_val},
            "new_value": None,
            "position": position,
        })

    return elements


def _get_color_channel(full_content, inputs_block, channel_name):
    """Get a color channel value, checking inline first then SourceOp spline."""
    val = get_input_number(inputs_block, channel_name)
    if val is not None:
        return val

    sourceop = get_input_sourceop(inputs_block, channel_name)
    if sourceop:
        spline_val = read_first_spline_value(full_content, sourceop)
        if spline_val is not None:
            return spline_val

    return 0.0


# ======================================================================
# Font Detection
# ======================================================================

def find_all_fonts(content, all_tools, logger):
    """
    Detect all font names used in the .comp file.
    Checks both inline inputs and BezierSpline keyframes.
    """
    fonts = set()

    # From inline Font inputs in text tools
    for name, tool_type, block, position in all_tools:
        if tool_type not in TEXT_TOOL_TYPES:
            continue
        inputs = find_inputs_block(block)
        font = get_input_string(inputs, "Font")
        if font:
            fonts.add(font)

    # From Font BezierSplines
    spline_pat = re.compile(r'\w+Font\s*=\s*BezierSpline\s*\{', re.MULTILINE)
    for match in spline_pat.finditer(content):
        block = extract_block(content, match.end())
        for font_name in re.findall(
            r'Value\s*=\s*Text\s*\{\s*Value\s*=\s*"([^"]+)"\s*\}', block
        ):
            fonts.add(font_name)

    return fonts


# ======================================================================
# Spline Helpers
# ======================================================================

def read_first_spline_value(content, spline_name):
    """Read the first numeric keyframe value from a named BezierSpline."""
    pattern = re.compile(
        rf'{re.escape(spline_name)}\s*=\s*BezierSpline\s*\{{', re.MULTILINE
    )
    m = pattern.search(content)
    if not m:
        return None
    block = extract_block(content, m.end())
    val_m = re.search(r'\[\s*[\d.]+\s*\]\s*=\s*\{\s*([\d.]+)', block)
    if val_m:
        return float(val_m.group(1))
    return None


# ======================================================================
# Color Helpers
# ======================================================================

def rgb_float_to_hex(r, g, b):
    ri = max(0, min(255, int(r * 255)))
    gi = max(0, min(255, int(g * 255)))
    bi = max(0, min(255, int(b * 255)))
    return f"#{ri:02x}{gi:02x}{bi:02x}"


# ======================================================================
# Main Parser
# ======================================================================

def parse_comp_file(filepath, logger):
    result = {
        "valid": False,
        "file_path": filepath,
        "file_size_kb": 0,
        "render_range": [0, 100],
        "fps": 30,
        "width": 1920,
        "height": 1080,
        "duration_frames": 100,
        "duration_seconds": 3.33,
        "raw_content": "",
        "changeable_elements": [],
        "all_fonts_used": [],
        "resolution": {"width": 1920, "height": 1080},
        "text_tools": [],
        "loader_tools": [],
    }

    if not os.path.isfile(filepath):
        logger.error(f"File not found: {filepath}")
        return result

    result["file_size_kb"] = round(os.path.getsize(filepath) / 1024, 2)
    logger.info(f"File: {filepath}")
    logger.info(f"Size: {result['file_size_kb']} KB")

    # Read file with encoding fallback
    content = None
    for encoding in ["utf-8-sig", "utf-8", "latin-1", "utf-16"]:
        try:
            with open(filepath, "r", encoding=encoding) as f:
                content = f.read()
            break
        except (UnicodeDecodeError, UnicodeError):
            continue

    if content is None:
        logger.error("Cannot read file with any known encoding.")
        return result

    result["raw_content"] = content

    if "Composition" not in content and "Tools" not in content:
        logger.error("File does not look like a Fusion .comp file.")
        return result

    result["valid"] = True
    logger.info("Valid Fusion .comp file detected.")

    # Global metadata
    start, end = _extract_render_range(content)
    result["render_range"] = [start, end]
    result["duration_frames"] = end - start + 1
    result["fps"] = _extract_fps(content)
    result["duration_seconds"] = round(result["duration_frames"] / result["fps"], 2)

    w, h = _extract_resolution(content)
    result["width"] = w
    result["height"] = h
    result["resolution"] = {"width": w, "height": h}

    logger.info(f"Range: {start}-{end} ({result['duration_frames']} frames)")
    logger.info(f"FPS: {result['fps']}")
    logger.info(f"Duration: {result['duration_seconds']}s")
    logger.info(f"Resolution: {w}x{h}")

    # Discover all tools
    all_tools = find_all_tools(content)
    logger.info(f"Tools discovered: {len(all_tools)}")
    for name, ttype, _, _ in all_tools:
        logger.info(f"  [{ttype}] {name}")

    # Collect elements
    elements = []

    # 1. Text from BezierSpline keyframes (animated)
    spline_texts = find_text_splines(content, logger)
    spline_tool_names = {e["tool_name"] for e in spline_texts}
    elements.extend(spline_texts)

    # 2. Text from inline TextPlus/Text3D (static)
    inline_texts = find_inline_text_tools(content, all_tools, spline_tool_names, logger)
    elements.extend(inline_texts)

    # 3. Image / video from Loader tools
    loaders = find_loader_tools(content, all_tools, logger)
    elements.extend(loaders)

    # 4. Image / video from MediaIn tools
    media_ins = find_mediain_tools(all_tools, logger)
    elements.extend(media_ins)

    # 5. Background colors
    backgrounds = find_background_tools(content, all_tools, logger)
    elements.extend(backgrounds)

    # 6. Text colors
    text_colors = find_text_color_elements(all_tools, logger)
    elements.extend(text_colors)

    # 7. Fonts
    fonts_used = find_all_fonts(content, all_tools, logger)
    result["all_fonts_used"] = sorted(fonts_used)

    # Number elements
    for i, elem in enumerate(elements):
        elem["index"] = i + 1

    result["changeable_elements"] = elements

    # Compatibility lists
    for e in elements:
        if e["type"] == "text":
            vals = e.get("current_values", [])
            text_val = vals[0] if vals else e.get("current_value", "")
            result["text_tools"].append({
                "name": e["tool_name"],
                "properties": {"styled_text": text_val}
            })
        elif e["type"] == "image":
            result["loader_tools"].append({
                "name": e["tool_name"],
                "properties": {"filename": e["current_value"]}
            })

    # Summary
    logger.info(f"Changeable elements: {len(elements)}")
    logger.info(f"  Text: {sum(1 for e in elements if e['type'] == 'text')}")
    logger.info(f"  Image/Video: {sum(1 for e in elements if e['type'] == 'image')}")
    logger.info(f"  Background color: {sum(1 for e in elements if e['type'] == 'color')}")
    logger.info(f"  Text color: {sum(1 for e in elements if e['type'] == 'text_color')}")
    logger.info(f"  Fonts used: {sorted(fonts_used)}")

    return result


# ======================================================================
# Metadata Extraction
# ======================================================================

def _extract_render_range(content):
    for pat in [
        r'RenderRange\s*=\s*\{\s*(\d+)\s*,\s*(\d+)\s*\}',
        r'GlobalRange\s*=\s*\{\s*(\d+)\s*,\s*(\d+)\s*\}',
    ]:
        m = re.search(pat, content)
        if m:
            return int(m.group(1)), int(m.group(2))
    return 0, 100


def _extract_fps(content):
    for pat in [r'\bFrameRate\s*=\s*(\d+)', r'\bFPS\s*=\s*(\d+)']:
        m = re.search(pat, content)
        if m:
            return int(m.group(1))
    return 30


def _extract_resolution(content):
    w, h = 1920, 1080
    wm = re.search(r'\bWidth\s*=\s*Input\s*\{\s*Value\s*=\s*(\d+)', content)
    hm = re.search(r'\bHeight\s*=\s*Input\s*\{\s*Value\s*=\s*(\d+)', content)
    if wm:
        w = int(wm.group(1))
    if hm:
        h = int(hm.group(1))
    return w, h