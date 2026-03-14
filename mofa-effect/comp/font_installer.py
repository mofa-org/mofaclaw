"""
MoFA Effect v2 - Font Installer

Downloads fonts from Google Fonts by the exact family names found in the .comp,
then installs them into C:\Windows\Fonts so that DaVinci Resolve can render them.

On Windows, font installation requires admin rights or using the per-user
registry trick. We try the per-user approach first (no admin needed), then
fall back to copying into C:\Windows\Fonts directly.
"""

import os
import re
import sys
import winreg
import shutil
import urllib.request

FONT_CACHE_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "output", "fonts")

STYLE_TO_WEIGHT = {
    "thin":       100,
    "extralight": 200,
    "light":      300,
    "regular":    400,
    "medium":     500,
    "semibold":   600,
    "bold":       700,
    "extrabold":  800,
    "black":      900,
}

# Google Fonts CSS API - requesting TTF via desktop user-agent
_UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"


def install_fonts_for_comp(splines, logger):
    """
    Given the parsed splines dict, find all font families + styles used,
    download them from Google Fonts, and install into Windows.
    Returns a dict of (family, style) -> installed_font_path.
    """
    os.makedirs(FONT_CACHE_DIR, exist_ok=True)
    logger.section("Font Download & Install")

    font_map = {}

    font_spline_names = [n for n in splines if n.endswith("Font")]
    for fsn in font_spline_names:
        base = fsn[:-4]
        style_spline = splines.get(f"{base}Style")
        font_spline  = splines.get(fsn)
        if font_spline is None:
            continue

        families = set()
        styles   = set()

        for _, v in getattr(font_spline, "keyframes", []):
            if v:
                families.add(v)

        if style_spline:
            for _, v in getattr(style_spline, "keyframes", []):
                if v:
                    styles.add(v)
        if not styles:
            styles = {"Regular"}

        for family in families:
            for style in styles:
                key = (family, style)
                if key in font_map:
                    continue
                path = _ensure_font(family, style, logger)
                font_map[key] = path

    logger.info(f"Font resolution complete: {len(font_map)} variant(s)")
    return font_map


def _ensure_font(family, style, logger):
    """
    Download (if needed) and install the font. Returns installed path or None.
    """
    weight = STYLE_TO_WEIGHT.get(style.lower(), 400)
    cache_name = re.sub(r'[^a-z0-9]', '', family.lower()) + f"_{weight}.ttf"
    cached_path = os.path.join(FONT_CACHE_DIR, cache_name)

    # Already downloaded?
    if not os.path.isfile(cached_path):
        logger.info(f'Downloading "{family}" w{weight} from Google Fonts...')
        ok = _download_font(family, weight, cached_path, logger)
        if not ok:
            logger.warning(f'  Download failed for "{family}" {style}. Resolve will use its own fallback.')
            return None

    # Install into Windows
    installed_path = _install_font_windows(cached_path, family, style, logger)
    return installed_path


def _download_font(family, weight, dest_path, logger):
    """Download a TTF from Google Fonts CSS API. Returns True on success."""
    family_param = family.replace(" ", "+")
    css_url = (
        f"https://fonts.googleapis.com/css2"
        f"?family={family_param}:wght@{weight}&display=swap"
    )

    try:
        req = urllib.request.Request(css_url, headers={"User-Agent": _UA})
        with urllib.request.urlopen(req, timeout=15) as resp:
            css = resp.read().decode("utf-8")

        # Google Fonts CSS contains  src: url(https://.../*.ttf) format('truetype')
        url_m = re.search(r'url\((https://[^)]+\.ttf)\)', css)
        if not url_m:
            # Sometimes it returns woff2 only — try anyway
            url_m = re.search(r'url\((https://[^)]+)\)', css)
        if not url_m:
            logger.warning(f'  No font URL found in CSS for "{family}" w{weight}')
            return False

        font_url = url_m.group(1)
        req2 = urllib.request.Request(font_url, headers={"User-Agent": _UA})
        with urllib.request.urlopen(req2, timeout=30) as resp2:
            data = resp2.read()

        with open(dest_path, "wb") as f:
            f.write(data)

        logger.info(f'  Saved: {os.path.basename(dest_path)} ({len(data)//1024} KB)')
        return True

    except Exception as e:
        logger.warning(f'  Google Fonts error: {e}')
        return False


def _install_font_windows(ttf_path, family, style, logger):
    """
    Install a TTF into Windows using the per-user font registry key
    (no admin rights required). Falls back to copying to C:\Windows\Fonts.
    Returns the path where the font was installed.
    """
    if sys.platform != "win32":
        # Non-Windows: just return the cached path (for testing)
        return ttf_path

    font_name = f"{family} {style}"
    ttf_filename = os.path.basename(ttf_path)

    # Strategy 1: per-user font installation via registry (no admin needed)
    per_user_fonts_dir = os.path.join(
        os.environ.get("LOCALAPPDATA", ""), "Microsoft", "Windows", "Fonts"
    )
    try:
        os.makedirs(per_user_fonts_dir, exist_ok=True)
        dest = os.path.join(per_user_fonts_dir, ttf_filename)
        if not os.path.isfile(dest):
            shutil.copy2(ttf_path, dest)

        # Register in per-user font registry
        reg_path = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"
        with winreg.OpenKey(
            winreg.HKEY_CURRENT_USER, reg_path, 0, winreg.KEY_SET_VALUE
        ) as key:
            winreg.SetValueEx(key, font_name, 0, winreg.REG_SZ, dest)

        logger.info(f'  Installed (per-user): "{font_name}" -> {dest}')
        return dest

    except Exception as e:
        logger.warning(f'  Per-user install failed: {e}. Trying system fonts...')

    # Strategy 2: copy to C:\Windows\Fonts (requires admin)
    system_fonts = r"C:\Windows\Fonts"
    try:
        dest = os.path.join(system_fonts, ttf_filename)
        if not os.path.isfile(dest):
            shutil.copy2(ttf_path, dest)

        reg_path = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"
        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE, reg_path, 0, winreg.KEY_SET_VALUE
        ) as key:
            winreg.SetValueEx(key, font_name, 0, winreg.REG_SZ, dest)

        logger.info(f'  Installed (system): "{font_name}" -> {dest}')
        return dest

    except PermissionError:
        logger.warning(
            f'  No admin rights to install to C:\\Windows\\Fonts. '
            f'Run as Administrator if Resolve cannot find "{font_name}".'
        )
        return ttf_path

    except Exception as e:
        logger.warning(f'  System install failed: {e}')
        return ttf_path