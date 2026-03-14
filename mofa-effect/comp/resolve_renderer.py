"""
MoFA Effect v2 - Resolve Renderer (internal script approach)

This module handles the handoff between main.py (which runs in your terminal)
and mofa_render.py (which runs inside Resolve via Workspace > Scripts).

Flow:
  main.py calls install_render_script()
    -> copies mofa_render.py into Resolve's Scripts/Comp/ folder
    -> writes a handoff JSON file with the comp path + output path
    -> opens Resolve and tells the user what to click
"""

import os
import sys
import json
import shutil
import subprocess

# Paths
RESOLVE_EXE         = r"E:\Softwares\Davinci\Resolve.exe"
RESOLVE_SCRIPTS_DIR = os.path.join(
    os.environ.get("APPDATA", r"C:\Users\Default\AppData\Roaming"),
    r"Blackmagic Design\DaVinci Resolve\Support\Fusion\Scripts\Utility"
)
SCRIPT_NAME     = "MoFA_Render.py"
# Write to %TEMP% — a fixed location Resolve can always reach without __file__
HANDOFF_PATH = os.path.join(os.environ.get("TEMP", r"C:\Temp"), "mofa_handoff.json")


def install_render_script(modified_comp, output_mp4, project_root, logger, comp_data=None):
    """
    1. Write handoff JSON so mofa_render.py knows what to render and where
    2. Copy mofa_render.py into Resolve's Scripts/Comp/ folder
    3. Launch Resolve (normal GUI mode)
    4. Print clear step-by-step instructions
    """
    logger.section("Installing Render Script into Resolve")

    # Step 1: Write handoff file
    handoff_path = HANDOFF_PATH
    handoff = {
        "comp_path":       modified_comp.replace("\\", "/"),
        "output_path":     output_mp4.replace("\\", "/"),
        "fps":             int(comp_data["fps"])             if comp_data else 30,
        "width":           int(comp_data["width"])           if comp_data else 1920,
        "height":          int(comp_data["height"])          if comp_data else 1080,
        "duration_frames": int(comp_data["duration_frames"]) if comp_data else 150,
    }
    os.makedirs(os.path.dirname(handoff_path), exist_ok=True)
    with open(handoff_path, "w") as f:
        json.dump(handoff, f, indent=2)
    logger.info(f"Handoff file written: {handoff_path}")

    # Step 2: Copy mofa_render.py to Resolve's Scripts folder
    source_script = os.path.join(project_root, "mofa_render.py")
    if not os.path.isfile(source_script):
        logger.error(f"mofa_render.py not found at: {source_script}")
        logger.error("Make sure mofa_render.py is in the same folder as main.py")
        return False

    os.makedirs(RESOLVE_SCRIPTS_DIR, exist_ok=True)
    dest_script = os.path.join(RESOLVE_SCRIPTS_DIR, SCRIPT_NAME)
    shutil.copy2(source_script, dest_script)
    logger.info(f"Script installed: {dest_script}")

    # Step 3: Launch Resolve if not already open
    _launch_resolve(logger)

    # Step 4: Instructions
    _print_instructions(output_mp4)
    return True


def _launch_resolve(logger):
    if not os.path.isfile(RESOLVE_EXE):
        logger.warning(f"Resolve.exe not found at: {RESOLVE_EXE}")
        logger.warning("Open DaVinci Resolve manually.")
        return
    try:
        # Check if already running
        result = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq Resolve.exe"],
            capture_output=True, text=True
        )
        if "Resolve.exe" in result.stdout:
            return  # Already running
        subprocess.Popen([RESOLVE_EXE])
    except Exception:
        pass  # Non-critical — user can open Resolve manually


def _print_instructions(output_mp4):
    # ASCII-only instructions to avoid encoding issues on Windows consoles.
    print("")
    print("==============================================================")
    print("  RESOLVE IS OPENING - FOLLOW THESE STEPS:")
    print("==============================================================")
    print("")
    print("  1. Wait for DaVinci Resolve to finish loading")
    print("")
    print("  2. In the menu bar, click:")
    print("     Workspace -> Scripts -> MoFA_Render")
    print("")
    print("  3. The script will run automatically:")
    print("     - Import your modified .comp")
    print("     - Set up the Deliver render")
    print("     - Render the video")
    print("     - Save the video file")
    print("")
    print("  4. Your output will be saved to:")
    print(f"     {output_mp4}")
    print("")
    print("  Watch the Console (Workspace -> Console) for progress.")
    print("==============================================================")