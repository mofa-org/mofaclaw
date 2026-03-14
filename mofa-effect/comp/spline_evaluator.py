"""
MoFA Effect v2 - BezierSpline Evaluator

Parses and evaluates Fusion BezierSpline keyframe data.

Two spline types:
  - Numeric splines (colors, alpha, blend, size): linearly interpolated
    between keyframes. Flags { Linear = true } confirms this on the segment.
    We use linear for all segments (user-confirmed).
  - Text/discrete splines (StyledText, Font, Style): step function.
    The value at keyframe K holds until the next keyframe.
    Identified by LockedY flag or Value = Text { ... } entries.
"""

import re


def extract_block(content, start):
    """Extract inner content of a { } block. start is just after opening {."""
    depth, i, n = 1, start, len(content)
    while i < n and depth > 0:
        c = content[i]
        if c == '{':
            depth += 1
        elif c == '}':
            depth -= 1
        i += 1
    return content[start:i - 1]


class NumericSpline:
    """
    A numeric animated property (color channel, alpha, blend, size...).
    Evaluates via linear interpolation between keyframes.
    Keyframe times are in SECONDS (Fusion stores t/fps, where fps=30 here,
    but the actual stored value is already in seconds).
    """

    def __init__(self, name, keyframes):
        # keyframes: list of (time_sec, value) sorted by time
        self.name = name
        self.keyframes = sorted(keyframes, key=lambda x: x[0])

    def evaluate(self, t_sec):
        """Return interpolated value at time t_sec."""
        kfs = self.keyframes
        if not kfs:
            return 0.0
        if t_sec <= kfs[0][0]:
            return kfs[0][1]
        if t_sec >= kfs[-1][0]:
            return kfs[-1][1]
        # Find surrounding keyframes
        for i in range(len(kfs) - 1):
            t0, v0 = kfs[i]
            t1, v1 = kfs[i + 1]
            if t0 <= t_sec <= t1:
                if t1 == t0:
                    return v0
                alpha = (t_sec - t0) / (t1 - t0)
                return v0 + alpha * (v1 - v0)
        return kfs[-1][1]


class TextSpline:
    """
    A discrete/text animated property (StyledText, Font, Style).
    Step function: holds the value of the last keyframe at or before t.
    """

    def __init__(self, name, keyframes):
        # keyframes: list of (time_sec, str_value) sorted by time
        self.name = name
        self.keyframes = sorted(keyframes, key=lambda x: x[0])

    def evaluate(self, t_sec):
        """Return string value at time t_sec (step function)."""
        kfs = self.keyframes
        if not kfs:
            return ""
        result = kfs[0][1]
        for t, v in kfs:
            if t <= t_sec:
                result = v
            else:
                break
        return result

    def all_values(self):
        """Return list of (time_sec, value) for all unique text transitions."""
        seen = []
        last = None
        for t, v in self.keyframes:
            if v != last:
                seen.append((t, v))
                last = v
        return seen


def parse_all_splines(comp_content):
    """
    Parse all BezierSplines from a .comp file.
    Returns dict: spline_name -> NumericSpline or TextSpline
    """
    splines = {}
    pattern = re.compile(r'(\w+)\s*=\s*BezierSpline\s*\{', re.MULTILINE)

    for match in pattern.finditer(comp_content):
        name = match.group(1)
        block = extract_block(comp_content, match.end())

        # Check if this is a text/discrete spline
        text_entries = re.findall(
            r'\[\s*([\d.]+)\s*\].*?Value\s*=\s*Text\s*\{\s*Value\s*=\s*"([^"]*)"\s*\}',
            block, re.DOTALL
        )

        if text_entries:
            kfs = [(float(t), v) for t, v in text_entries]
            # Also check for empty-string initial keyframe (numeric 0 value with no Text)
            # Some splines have [t] = { 0, ... } before the first Text entry
            numeric_before = re.findall(
                r'\[\s*([\d.]+)\s*\]\s*=\s*\{\s*([\d.]+)[^}]*Flags\s*=\s*\{[^}]*LockedY',
                block
            )
            # Add numeric-indexed entries that precede text entries as empty
            for t_str, _ in numeric_before:
                t = float(t_str)
                if not any(abs(k[0] - t) < 0.01 for k in kfs):
                    kfs.append((t, ""))
            splines[name] = TextSpline(name, kfs)
        else:
            # Numeric spline - parse [time] = { value, ... }
            kf_pattern = re.compile(
                r'\[\s*([\d.]+)\s*\]\s*=\s*\{\s*([\d.]+)'
            )
            kfs = []
            for m in kf_pattern.finditer(block):
                t = float(m.group(1))
                v = float(m.group(2))
                kfs.append((t, v))
            if kfs:
                splines[name] = NumericSpline(name, kfs)

    return splines