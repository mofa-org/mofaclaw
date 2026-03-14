"""
MoFA Effect v2 - mofa_render.py
Runs INSIDE DaVinci Resolve via Workspace > Scripts > Utility > MoFA_Render

Strategy (confirmed working approach from Resolve scripting API):
  1. Write a tiny black PNG to %TEMP% (pure Python, no deps)
  2. Import it into the Media Pool  (ImportMedia DOES work for image files)
  3. Put it on the timeline        (AppendToTimeline -> gives us a TimelineItem)
  4. ImportFusionComp(path)        (loads our .comp onto that TimelineItem)
  5. Render via Deliver page
"""

import os
import json
import time
import struct
import zlib
import base64
import datetime


# ── Tiny 8x8 black PNG — embedded so we need zero external files ──────────────
_BLACK_PNG_B64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAADElEQVR4nGNg"
    "GB4AAADIAAGtQHYiAAAAAElFTkSuQmCC"
)


def _write_black_png(path):
    data = base64.b64decode(_BLACK_PNG_B64)
    with open(path, "wb") as f:
        f.write(data)


def run():
    print("=" * 55)
    print("  MoFA Effect v2 - Resolve Render Script")
    print("=" * 55)

    # ── Read handoff ──────────────────────────────────────────────────────────
    handoff_path = os.path.join(os.environ.get("TEMP", r"C:\Temp"), "mofa_handoff.json")
    if not os.path.isfile(handoff_path):
        print(f"[ERROR] Handoff not found: {handoff_path}")
        print("  Run main.py first.")
        return

    with open(handoff_path, "r") as f:
        handoff = json.load(f)

    comp_path       = handoff["comp_path"].replace("/", "\\")
    output_path     = handoff["output_path"].replace("/", "\\")
    fps             = int(handoff.get("fps", 30))
    width           = int(handoff.get("width", 1920))
    height          = int(handoff.get("height", 1080))
    duration_frames = int(handoff.get("duration_frames", 150))

    print(f"Comp    : {comp_path}")
    print(f"Output  : {output_path}")
    print(f"Size    : {width}x{height} @ {fps}fps  ({duration_frames} frames)")
    print()

    if not os.path.isfile(comp_path):
        print(f"[ERROR] Comp not found: {comp_path}")
        return

    # ── Get resolve ───────────────────────────────────────────────────────────
    try:
        r = resolve  # noqa — injected by Resolve
    except NameError:
        print("[ERROR] 'resolve' not available. Run via Workspace > Scripts.")
        return

    # ── Create project ────────────────────────────────────────────────────────
    pm = r.GetProjectManager()
    ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    project_name = f"MoFA_{ts}"

    project = pm.CreateProject(project_name)
    if project is None:
        print(f"[ERROR] Could not create project '{project_name}'.")
        return
    print(f"Project : {project_name}")

    project.SetSetting("timelineFrameRate",        str(fps))
    project.SetSetting("timelineResolutionWidth",  str(width))
    project.SetSetting("timelineResolutionHeight", str(height))

    media_pool = project.GetMediaPool()

    # ── Write the black PNG to %TEMP% and import it ───────────────────────────
    # ImportMedia works fine for image files — just not for .comp files.
    # This gives us a real MediaPoolItem we can place on the timeline.
    tmp_png = os.path.join(os.environ.get("TEMP", r"C:\Temp"), "mofa_black.png")
    _write_black_png(tmp_png)
    print(f"Importing placeholder image: {tmp_png}")

    png_fwd = tmp_png.replace("\\", "/")
    items = media_pool.ImportMedia([png_fwd])
    if not items:
        print("[ERROR] ImportMedia failed for placeholder PNG.")
        print("  This is unexpected — even Resolve Free can import PNGs.")
        _cleanup(pm, project, project_name)
        return

    png_item = items[0]

    # Set its clip duration to match our comp
    png_item.SetClipProperty("End", str(duration_frames - 1))
    print(f"  PNG media pool item created ({duration_frames} frames)")

    # ── Create timeline and put the PNG on it ─────────────────────────────────
    timeline = media_pool.CreateEmptyTimeline(project_name)
    if timeline is None:
        print("[ERROR] CreateEmptyTimeline failed.")
        _cleanup(pm, project, project_name)
        return
    timeline.SetSetting("timelineFrameRate", str(fps))
    project.SetCurrentTimeline(timeline)
    print(f"Timeline: {timeline.GetName()}")

    result = media_pool.AppendToTimeline([{
        "mediaPoolItem": png_item,
        "startFrame":    0,
        "endFrame":      duration_frames - 1,
        "mediaType":     1,   # video only
    }])
    if not result:
        print("[ERROR] AppendToTimeline failed.")
        _cleanup(pm, project, project_name)
        return

    # ── Get the timeline item ─────────────────────────────────────────────────
    r.OpenPage("edit")
    items_in_track = timeline.GetItemListInTrack("video", 1)
    if not items_in_track:
        print("[ERROR] No items on video track 1 after AppendToTimeline.")
        _cleanup(pm, project, project_name)
        return

    timeline_item = items_in_track[0]
    print(f"Timeline item: {timeline_item.GetName()}")

    # ── Import our .comp onto the timeline item ───────────────────────────────
    print(f"Loading comp: {os.path.basename(comp_path)}")
    comp_fwd = comp_path.replace("\\", "/")
    fusion_comp = timeline_item.ImportFusionComp(comp_fwd)

    if fusion_comp is None:
        print(f"[ERROR] ImportFusionComp() failed.")
        print(f"  Path tried: {comp_fwd}")
        print("  Check the .comp file is valid and not locked.")
        _cleanup(pm, project, project_name)
        return

    print("  Fusion comp loaded OK.")

    # ── Deliver page render ───────────────────────────────────────────────────
    r.OpenPage("deliver")

    output_dir  = os.path.dirname(output_path)
    output_name = os.path.splitext(os.path.basename(output_path))[0]
    os.makedirs(output_dir, exist_ok=True)

    project.SetRenderSettings({
        "SelectAllFrames": True,
        "TargetDir":       output_dir.replace("\\", "/"),
        "CustomName":      output_name,
        "ExportVideo":     True,
        "ExportAudio":     False,
        "FormatWidth":     width,
        "FormatHeight":    height,
        "VideoQuality":    0,
        "Format":          "MP4",
        "Codec":           "H264",
    })

    job_id = project.AddRenderJob()
    if not job_id:
        print("[ERROR] AddRenderJob() failed.")
        _cleanup(pm, project, project_name)
        return
    print(f"Render job: {job_id}\n")

    if not project.StartRendering(job_id):
        print("[ERROR] StartRendering() returned False.")
        _cleanup(pm, project, project_name)
        return

    print("Rendering...")
    while True:
        time.sleep(2)
        status = project.GetRenderJobStatus(job_id)
        if status is None:
            print("[ERROR] Lost render status.")
            break

        state      = status.get("JobStatus", "")
        completion = status.get("CompletionPercentage", 0)
        print(f"  [{state}]  {completion:.0f}%")

        if state == "Complete":
            print()
            print("=" * 55)
            print("  RENDER COMPLETE")
            print(f"  {output_path}")
            if os.path.isfile(output_path):
                mb = os.path.getsize(output_path) / (1024 * 1024)
                print(f"  {mb:.2f} MB")
            print("=" * 55)
            break

        if state in ("Failed", "Cancelled"):
            print(f"[ERROR] Render {state}: {status.get('Error', 'no details')}")
            break

    _cleanup(pm, project, project_name)


def _cleanup(pm, project, project_name):
    try:
        pm.CloseProject(project)
        pm.DeleteProject(project_name)
        print(f"Cleaned up: {project_name}")
    except Exception as e:
        print(f"[WARN] Cleanup: {e}")


run()