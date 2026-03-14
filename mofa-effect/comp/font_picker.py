"""
MoFA Effect v2 - Font Picker

Scans the Windows Fonts folder for installed fonts, presents a clean
numbered list, and returns the chosen font name exactly as Windows
knows it (so Resolve/Fusion can find it without errors).

Usage:
    from font_picker import pick_font
    chosen = pick_font()   # returns e.g. "Segoe UI" or "Arial"
"""

import os
import re


# Fonts confirmed reliable in Resolve's Fusion / Text+ engine on Windows.
# Listed roughly from most to least reliable in Resolve's internal font registry.
WINDOWS_SAFE = [
    "Arial",          # Most reliable — always works in Resolve Text+
    "Arial Black",
    "Impact",         # Very reliable in Resolve
    "Tahoma",
    "Verdana",
    "Georgia",
    "Times New Roman",
    "Courier New",
    "Trebuchet MS",
    "Comic Sans MS",
    "Calibri",        # Usually works but has known Resolve registry issues
    "Cambria",
    "Consolas",
    "Constantia",
    "Corbel",
    "Franklin Gothic Medium",
    "Segoe UI",
]

WINDOWS_FONTS_DIR = r"C:\Windows\Fonts"


def get_installed_fonts():
    """
    Return a sorted list of font family names installed on this machine.
    Scans C:\\Windows\\Fonts for .ttf / .otf files and extracts clean names.
    Falls back to the safe list if the Fonts folder can't be read.
    """
    if not os.path.isdir(WINDOWS_FONTS_DIR):
        return WINDOWS_SAFE[:]

    seen = set()
    names = []

    for fname in sorted(os.listdir(WINDOWS_FONTS_DIR)):
        if not fname.lower().endswith((".ttf", ".otf")):
            continue

        # Strip extension and common weight/style suffixes to get family name
        base = os.path.splitext(fname)[0]
        # Remove trailing weight/style tokens like -Bold, _Italic, Bold, Regular, etc.
        family = re.sub(
            r'[-_\s]+(Bold|Italic|Regular|Light|Thin|Black|Medium|'
            r'SemiBold|ExtraBold|Heavy|Condensed|Narrow|Oblique|'
            r'BoldItalic|LightItalic|MediumItalic).*$',
            '', base, flags=re.IGNORECASE
        ).strip()
        # Normalise spaces (some files use CamelCase)
        family = re.sub(r'(?<=[a-z])(?=[A-Z])', ' ', family)
        family = re.sub(r'\s+', ' ', family).strip()

        if family and family not in seen:
            seen.add(family)
            names.append(family)

    return sorted(names) if names else WINDOWS_SAFE[:]


def pick_font(prompt="Choose a font for your text"):
    """
    Interactive font picker. Prints installed fonts in columns with numbers.
    The safe/common fonts are listed first with a [*] marker.
    Returns the chosen font name string.
    """
    installed = get_installed_fonts()

    # Split into safe-first + others
    safe_set   = set(WINDOWS_SAFE)
    safe_fonts = [f for f in installed if f in safe_set]
    other_fonts = [f for f in installed if f not in safe_set]

    all_fonts = safe_fonts + other_fonts

    print(f"\n  {prompt}")
    print()
    print("  IMPORTANT: Resolve's Text+ has a known font registry bug.")
    print("     Even fonts installed on Windows may not be found by Resolve's Fusion engine.")
    print("     Fonts marked [*] are the safest choices and are built into Resolve.")
    print("     If you pick another font and see 'Could not find font' errors,")
    print("     re-run main.py and choose a [*] font instead.")
    print()

    # Print in 2 columns
    col_width = 36
    for i, name in enumerate(all_fonts):
        marker = "[*]" if name in safe_set else "   "
        entry  = f"  {i+1:3}. {marker} {name}"
        if i % 2 == 0:
            print(entry.ljust(col_width * 2), end="")
        else:
            print(entry)
    if len(all_fonts) % 2 != 0:
        print()  # newline after odd last entry

    print()

    while True:
        raw = input(f"  Enter number (1-{len(all_fonts)}) or type a font name directly: ").strip()
        if not raw:
            chosen = "Arial"
            print(f"  -> No input. Using default: {chosen}")
            return chosen

        # Numeric choice
        if raw.isdigit():
            idx = int(raw) - 1
            if 0 <= idx < len(all_fonts):
                chosen = all_fonts[idx]
                print(f"  -> {chosen}")
                return chosen
            else:
                print(f"  -> Out of range. Enter 1-{len(all_fonts)}.")
                continue

        # Direct name entry — validate it exists in the list (case-insensitive)
        lower_map = {f.lower(): f for f in all_fonts}
        if raw.lower() in lower_map:
            chosen = lower_map[raw.lower()]
            print(f"  -> {chosen}")
            return chosen

        # Accept it anyway — user may know a font we didn't detect
        print(f"  -> Using '{raw}' (not detected in C:\\Windows\\Fonts — make sure it's installed)")
        return raw