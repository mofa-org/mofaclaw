# MoFA Effect Discord Workflow Guide

This guide walks through **everything** you need to:

- Run the Mofaclaw gateway with the Discord bot
- Trigger a **MoFA Effect** session via `/mofa-effect`
- Interactively edit a `.comp` template from Discord
- Render the final video in DaVinci Resolve
- Receive a **local HTTP link** to the rendered video

> This guide assumes:
> - You’re on **Windows** with **DaVinci Resolve** installed
> - You have **Python 3.11** in a virtualenv used for MoFA Effect
> - You have a **Discord bot** already configured in `mofaclaw` (the standard gateway Discord setup)

---

## 1. Project Layout & Key Paths

Repo root (simplified):

- `mofa-effect/comp/`
  - `main.py` – MoFA Effect terminal entrypoint
  - `resolve_renderer.py` – handoff into Resolve (`mofa_render.py` script install + handoff JSON)
  - `mofa_render.py` – Resolve-side script that actually renders the video
  - `output/` – modified `.comp` files, generated media, logs, and final video outputs
    - `images/`, `audio/`, `frames/`, `logs/`
  - `Guide.md` – **this file**
- `core/src/channels/discord/mod.rs` – Discord channel implementation (slash commands, message handler)

MoFA Effect uses its own `requirements.txt`:

- `mofa-effect/comp/requirements.txt`

The Discord integration is part of the **Rust gateway**:

- `cargo run -- gateway` (from repo root) starts the gateway and Discord bot.

---

## 2. One-time Setup

### 2.1 Python environment for MoFA Effect

Create and activate a venv (if not already done):

```bash
cd path\to\mofaclaw

# Using Anaconda, venv, or similar – example with venv:
python -m venv .mofaEffect

# Activate (PowerShell)
.\.mofaEffect\Scripts\Activate.ps1

# Or cmd
.\.mofaEffect\Scripts\activate.bat
```

Install MoFA Effect dependencies:

```bash
pip install -r mofa-effect/comp/requirements.txt
```

> Important: **Always** start the Rust gateway from a shell where this venv is activated so the `python` command points to this environment.

### 2.2 Rust & Gateway

Install Rust (if needed) and build:

```bash
cd path\to\mofaclaw
cargo build
```

Ensure your `~/.mofaclaw/config.json` has Discord configured and enabled (as per the standard Mofaclaw docs), so `cargo run -- gateway` will start the Discord bot.

---

## 3. Starting the Gateway + Discord Bot

From the **activated venv shell**:

```bash
cd path\to\mofaclaw
cargo run -- gateway
```

This:

- Starts the Mofaclaw gateway (LLM agent, MessageBus, channels).
- Starts the integrated Discord bot (via `DiscordChannel`) with slash commands.
- Ensures the `/mofa-effect` slash command is available in Discord.

---

## 4. What `/mofa-effect` Does

When you run `/mofa-effect` in a Discord channel:

1. The Discord gateway:
   - Spawns **one** `python main.py` process in `mofa-effect/comp` for that channel using the venv’s `python`.
   - Captures its **stdin/stdout/stderr**.
   - Registers a **MoFA session** for that channel:
     - While active, any non-bot messages in that channel are treated as input to `main.py`.
2. All `main.py` output is streamed back to the Discord channel as grouped code blocks.
3. After MoFA setup finishes:
   - The bot posts an explicit message telling you to run `MoFA_Render` inside Resolve.
   - The bot then watches for the final video file and posts an HTTP link once it appears.

> While a MoFA session is active for a channel, that channel is effectively a **remote terminal** for `main.py`.

---

## 5. Running a Full MoFA Session via Discord

### 5.1 Trigger from Discord

In a channel where the bot is present:

1. Type `/mofa-effect` and run it.
2. The bot will respond with messages showing:
   - MoFA banner
   - PHASE 1: Parse
   - PHASE 2: Splines
   - PHASE 3: Interactive Editing

### 5.2 Select the `.comp` Template

MoFA will ask (mirrored into Discord as log text):

```text
Comp file path:
> _
```

You reply in the channel with the full path to your `.comp` file, e.g.:

```text
E:\For-PR\Mofa-CLAW\mofaclaw\mofa-effect\comp\testing_files\09 Title.comp
```

MoFA parses the template and prints a summary:

- Duration, FPS, resolution
- Detected tools
- Detected changeable elements (text, image/video, colors, etc.)

### 5.3 Interactive Editing (PHASE 3)

MoFA prints a section like:

```text
PHASE 3: Interactive Editing
...
DETECTED ELEMENTS
  1. [TEXT] "Text1"
     Current values : [...]
  2. [IMAGE] "MediaIn1"
     Current file   : ... [MISSING - will use placeholder]
  3. [COLOR] "Background1"
...
Make changes? (yes/no) [yes]:
> _
```

Respond in Discord:

- `yes` – go through each element interactively.
- `no` – skip manual changes; MoFA will still generate placeholders for missing media.

#### 5.3.1 Changing Text

For text elements:

```text
[TEXT] "Text1"
  Current: ['GET', 'A NEW', 'STYLE', 'WITH']
  New (e.g. WELCOME / TO / MOFA, or Enter to keep):
> _
```

- Reply with something like:

  ```text
  WELCOME / TO / MOFA / MOFA
  ```

  This sets the animated text sequence.

- To keep as-is, just reply with an **empty line** in a real terminal; in Discord, reply with the same text again or some explicit “keep” scheme that you visually track (MoFA treats an empty string as keep).

#### 5.3.2 Changing Images / Placeholders

For image/video elements:

```text
[IMAGE] "MediaIn1"
  Current: /path/to/original.png
  Your file path (or type 'None' for placeholder):
> _
```

- Supply a new file path:

  ```text
  E:\Assets\logos\mofa-logo.png
  ```

- Or type one of:
  - `None`
  - `placeholder`
  - `skip`

  to **force MoFA to generate a placeholder** (even if the original file exists).

If the original file is missing and you just press Enter in a real terminal, MoFA will auto-generate a placeholder. From Discord, use `None`/`placeholder` explicitly.

#### 5.3.3 Changing Colors

For color and text-color elements:

```text
[COLOR] "Background1"
  Current: #f54545
  New hex color, e.g. #ff3300 (Enter to keep):
> _
```

- Reply with a hex like:

  ```text
  #00ff00
  ```

- Or reply with nothing (terminal) to keep; from Discord you typically just repeat the current value if you want it unchanged.

### 5.4 Font Selection (PHASE 4)

MoFA then prints:

```text
PHASE 4: Font Selection
...
Choose the font to use for all text in this video

IMPORTANT: Resolve's Text+ has a known font registry bug.
...
Enter number (1-N) or type a font name directly:
> _
```

You can:

- Reply with a **number** from the list (e.g. `1` for a safe font like Arial).
- Reply with a font name (e.g. `Roboto`).

MoFA uses this replacement font across text tools/splines.

### 5.5 Patch and Handoff (PHASE 5 & 6)

MoFA:

1. Applies your changes and writes a modified `.comp` into:

   ```text
   mofa-effect/comp/output/<basename>_mofa_<timestamp>.comp
   ```

2. Writes a handoff file (`mofa_handoff.json`) in `%TEMP%` with:
   - modified `.comp` path
   - expected output video path
   - fps, width, height, duration

3. Copies `mofa_render.py` into Resolve’s Scripts directory.
4. Launches Resolve (if not already open).
5. Prints ASCII instructions for what to click in Resolve.
6. Writes `render_report.json` into `mofa-effect/comp/output/` with:
   - `comp`, `modified_comp`, `expected_output`, etc.

The bot mirrors these logs and then sends:

```text
MoFA setup complete. In DaVinci Resolve, go to `Workspace → Scripts → MoFA_Render` to start rendering.
I will watch for the rendered video file and post a link here once it is ready.
```

At this point:

- The **Python MoFA process exits**.
- The Discord side:
  - Removes the active session mapping for that channel.
  - Starts (or reuses) `python -m http.server 8000` in `mofa-effect/`.
  - Begins polling for the final video file.

---

## 6. Rendering in Resolve & Getting the Link

### 6.1 Run MoFA_Render script in Resolve

In DaVinci Resolve:

1. Wait until it fully loads.
2. Use the menu:

   ```text
   Workspace -> Scripts -> MoFA_Render
   ```

3. The script will:
   - Read `mofa_handoff.json` from `%TEMP%`.
   - Import the modified `.comp`.
   - Set up the Deliver render.
   - Render the video.
   - Save it to `output_mp4` from the handoff JSON (often under `mofa-effect/comp/output/`).

### 6.2 Discord watcher logic

On the Rust side, once `render_report.json` exists, the watcher:

1. Reads `expected_output` from `render_report.json`.
2. Waits up to **1 hour** for:
   - Either that exact file to appear and be non-zero size, or
   - Any newest file in the output directory whose name matches `*_mofa_*` with extension `.mp4` or `.mov`.
3. When a video file is found:
   - Computes a relative path from `mofa-effect/` (so `python -m http.server` can serve it).
   - Posts in the same Discord channel:

     ```text
     Here is your video link:
     http://localhost:8000/comp/output/your_video_name.mov
     ```

If nothing appears within an hour, it posts a timeout message instead.

---

## 7. Troubleshooting

- **No link appears in Discord**
  - Check that:
    - `render_report.json` exists in `mofa-effect/comp/output/`.
    - The final video file exists in `mofa-effect/comp/output/`.
  - Ensure the gateway is still running (`cargo run -- gateway`) in the same venv shell.
  - Confirm that the channel you used for `/mofa-effect` is the same one you’re watching.

- **Encoding errors / weird characters in logs**
  - All user-facing banners/instructions have been converted to ASCII to avoid Windows code page issues.
  - If you see another `UnicodeEncodeError`, note the character/location and adjust the corresponding print text similarly.

- **Multiple sessions / overlapping runs**
  - Only **one MoFA session per channel** is allowed. If you try `/mofa-effect` while one is active, the bot will tell you.
  - Use separate channels or wait for the previous session to finish.

---

## 8. Quick Reference: End-to-End Steps

1. Activate Python venv used by MoFA Effect.
2. From repo root, run:

   ```bash
   cargo run -- gateway
   ```

3. In Discord:
   - `/mofa-effect`
   - Provide `.comp` path.
   - Answer interactive edits for text, image (paths or `None`), colors.
   - Choose a font by number or name.

4. Wait for the bot to say:

   > MoFA setup complete. In DaVinci Resolve, go to `Workspace → Scripts → MoFA_Render` to start rendering.

5. In Resolve:
   - `Workspace -> Scripts -> MoFA_Render`
   - Let it finish rendering.

6. Back in Discord:
   - Wait for:

     ```text
     Here is your video link:
     http://localhost:8000/comp/output/your_video_name.mov
     ```

   - Open that URL from the same machine/network where `python -m http.server` is running.

