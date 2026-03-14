import asyncio
import logging
import os
import sys
from functools import partial
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from pathlib import Path
from typing import Optional

import discord
from discord.ext import commands

from .config import load_discord_settings, load_http_settings
from .session_store import GLOBAL_SESSION_STORE, Session


logger = logging.getLogger("mofa.discord_bot")


# Make MoFA Effect comp modules importable by adding the comp directory to sys.path.
COMP_DIR = Path(__file__).resolve().parents[1] / "mofa-effect" / "comp"
if COMP_DIR.is_dir():
    sys.path.insert(0, str(COMP_DIR))

try:
    from headless_session import HeadlessSessionInputs, run_headless_session  # type: ignore
    from config import OUTPUT_DIR, LOGS_DIR  # type: ignore
    from utils import Logger  # type: ignore
    from comp_parser import parse_comp_file  # type: ignore
except Exception:
    # These imports will fail in environments where the comp folder is not available;
    # we log this at runtime when the command is actually used.
    HeadlessSessionInputs = None  # type: ignore
    run_headless_session = None  # type: ignore
    OUTPUT_DIR = None  # type: ignore
    LOGS_DIR = None  # type: ignore
    Logger = None  # type: ignore
    parse_comp_file = None  # type: ignore


def _configure_logging() -> None:
    level = os.getenv("MOFA_DISCORD_LOG_LEVEL", "INFO").upper()
    logging.basicConfig(
        level=getattr(logging, level, logging.INFO),
        format="[%(asctime)s] [%(levelname)s] %(name)s: %(message)s",
    )


def create_bot() -> commands.Bot:
    intents = discord.Intents.default()
    # Required to read user replies when we build multi-step flows.
    intents.message_content = True

    bot = commands.Bot(command_prefix="!", intents=intents)

    @bot.event
    async def on_ready() -> None:
        logger.info("Logged in as %s (ID: %s)", bot.user, bot.user.id if bot.user else "unknown")
        logger.info("Syncing application commands...")

        settings = load_discord_settings()

        # If guild IDs are provided, sync commands per guild for faster propagation.
        if settings.guild_ids:
            for gid in settings.guild_ids:
                guild = discord.Object(id=gid)
                await bot.tree.sync(guild=guild)
                logger.info("Synced commands for guild %s", gid)
        else:
            await bot.tree.sync()
            logger.info("Synced global application commands.")

    @bot.tree.command(name="mofa_effect", description="Run a MoFA Effect session on a .comp file")
    async def mofa_effect(
        interaction: discord.Interaction,
        comp: discord.Attachment,
    ) -> None:
        """
        Slash command entrypoint for running a MoFA Effect session.

        Flow:
          1) User uploads a .comp template.
          2) Bot parses changeable elements.
          3) Bot asks sequential questions for text/colors (images default to placeholders).
          4) Bot runs the headless MoFA pipeline.
          5) Separate background logic (added later) will watch for the MP4 and send a link.
        """
        if HeadlessSessionInputs is None or run_headless_session is None:
            await interaction.response.send_message(
                "MoFA Effect modules are not available on this host. "
                "Ensure the `mofa-effect/comp` directory is present.",
                ephemeral=True,
            )
            return

        await interaction.response.defer(ephemeral=True, thinking=True)

        # Persist the uploaded .comp into the MoFA output area.
        comp_filename = comp.filename or "template.comp"
        if not comp_filename.lower().endswith(".comp"):
            await interaction.followup.send(
                "Please upload a Fusion `.comp` file.", ephemeral=True
            )
            return

        uploads_dir = Path(OUTPUT_DIR) / "discord_uploads"  # type: ignore[arg-type]
        uploads_dir.mkdir(parents=True, exist_ok=True)
        target_path = uploads_dir / comp_filename
        await comp.save(target_path)

        # Create a new session record.
        session: Session = GLOBAL_SESSION_STORE.create(
            user_id=interaction.user.id,
            guild_id=interaction.guild.id if interaction.guild else None,
            channel_id=interaction.channel_id,  # type: ignore[arg-type]
            comp_path=str(target_path),
        )

        await interaction.followup.send(
            f"MoFA session `{session.id}` created. Parsing template...", ephemeral=True
        )

        # Parse template to discover changeable elements.
        logger_obj = Logger(LOGS_DIR)  # type: ignore[call-arg]
        parsed = parse_comp_file(str(target_path), logger_obj)  # type: ignore[call-arg]
        elements = parsed.get("changeable_elements", [])
        if not elements:
            await interaction.followup.send(
                "No changeable elements detected in this template. "
                "Nothing to edit; you can run it directly from the CLI if needed.",
                ephemeral=True,
            )
            GLOBAL_SESSION_STORE.delete(session.id)
            return

        # Present a brief summary.
        summary_lines = []
        for e in elements[:10]:
            idx = e.get("index")
            etype = e.get("type")
            name = e.get("tool_name")
            current = e.get("current_value", "")
            summary_lines.append(f"- #{idx} [{etype}] **{name}** → {current}")
        more = ""
        if len(elements) > 10:
            more = f"\n...and {len(elements) - 10} more elements."

        await interaction.followup.send(
            "Detected the following editable elements (first 10):\n"
            + "\n".join(summary_lines)
            + more,
            ephemeral=True,
        )

        # Sequentially collect overrides for text and color elements.
        overrides: dict[int, str] = {}

        async def ask_user(prompt: str) -> Optional[discord.Message]:
            await interaction.followup.send(prompt, ephemeral=True)

            def check(msg: discord.Message) -> bool:
                return (
                    msg.author.id == interaction.user.id
                    and msg.channel.id == interaction.channel_id
                )

            try:
                msg = await bot.wait_for("message", timeout=300.0, check=check)
            except asyncio.TimeoutError:
                await interaction.followup.send(
                    "Timed out waiting for a response. Session cancelled.",
                    ephemeral=True,
                )
                return None
            return msg

        for e in elements:
            idx = int(e.get("index", 0))
            if not idx:
                continue

            etype = e.get("type")
            name = e.get("tool_name")
            current = e.get("current_value", "")

            if etype == "text":
                msg = await ask_user(
                    f"Element #{idx} [TEXT] **{name}**\n"
                    f"Current: `{current}`\n"
                    "Reply with new text, or `skip` to keep as-is."
                )
                if msg is None:
                    GLOBAL_SESSION_STORE.delete(session.id)
                    return
                content = msg.content.strip()
                if not content or content.lower() == "skip":
                    continue
                overrides[idx] = content

            elif etype in ("color", "text_color"):
                label = "COLOR" if etype == "color" else "TEXT COLOR"
                msg = await ask_user(
                    f"Element #{idx} [{label}] **{name}**\n"
                    f"Current: `{current}`\n"
                    "Reply with a new hex color like `#ff3300`, or `skip` to keep."
                )
                if msg is None:
                    GLOBAL_SESSION_STORE.delete(session.id)
                    return
                content = msg.content.strip()
                if not content or content.lower() == "skip":
                    continue
                overrides[idx] = content

            # For now, image/video elements rely on auto-placeholder logic.

        # Font selection
        font_prompt = (
            "Font selection:\n"
            "Type the font name you want to use for all text in this video\n"
            "(e.g. `Arial`). Send an empty message or `default` to use a safe default."
        )
        msg = await ask_user(font_prompt)
        if msg is None:
            GLOBAL_SESSION_STORE.delete(session.id)
            return
        font_raw = msg.content.strip()
        font_name: Optional[str]
        if not font_raw or font_raw.lower() == "default":
            font_name = None
        else:
            font_name = font_raw

        session.element_overrides = overrides
        session.font_name = font_name
        session.status = "processing"
        GLOBAL_SESSION_STORE.update(session)

        await interaction.followup.send(
            "Running MoFA Effect headless session. "
            "I will let you know when Resolve is ready and when the video file appears.",
            ephemeral=True,
        )

        # Run the headless MoFA pipeline in a worker thread to avoid blocking the event loop.
        loop = asyncio.get_running_loop()
        inputs = HeadlessSessionInputs(  # type: ignore[call-arg]
            comp_path=str(target_path),
            element_overrides=session.element_overrides,
            font_name=session.font_name,
            auto_placeholder_missing=True,
        )
        logger_obj = Logger(LOGS_DIR)  # type: ignore[call-arg]

        try:
            result = await loop.run_in_executor(
                None, partial(run_headless_session, inputs, logger_obj)  # type: ignore[arg-type]
            )
        except Exception as exc:  # noqa: BLE001
            logger.exception("Error during headless MoFA session: %s", exc)
            session.status = "error"
            GLOBAL_SESSION_STORE.update(session)
            await interaction.followup.send(
                f"Headless MoFA session failed: {exc}", ephemeral=True
            )
            return

        session.expected_output_mp4 = result.expected_output_mp4  # type: ignore[attr-defined]
        session.status = "waiting_render"
        GLOBAL_SESSION_STORE.update(session)

        # Background task: watch for the rendered MP4 and post a link when available.
        async def watch_render_and_notify(s: Session) -> None:
            from .config import load_http_settings as _load_http_settings

            http_conf = _load_http_settings()
            max_wait_seconds = 60 * 60  # 1 hour
            poll_interval = 10.0

            start_time = asyncio.get_event_loop().time()
            output_path = s.expected_output_mp4
            if not output_path:
                return

            await asyncio.sleep(poll_interval)

            while True:
                now = asyncio.get_event_loop().time()
                if now - start_time > max_wait_seconds:
                    s.status = "error"
                    GLOBAL_SESSION_STORE.update(s)
                    try:
                        channel = bot.get_channel(s.channel_id)
                        if isinstance(channel, discord.TextChannel):
                            await channel.send(
                                f"MoFA session `{s.id}`: timed out waiting for rendered MP4."
                            )
                    except Exception:  # noqa: BLE001
                        logger.exception("Failed to send timeout message")
                    return

                if os.path.isfile(output_path) and os.path.getsize(output_path) > 0:
                    s.status = "done"
                    GLOBAL_SESSION_STORE.update(s)
                    rel_name = os.path.basename(output_path)
                    url = f"http://{http_conf.host}:{http_conf.port}/{rel_name}"
                    try:
                        channel = bot.get_channel(s.channel_id)
                        if isinstance(channel, discord.TextChannel):
                            await channel.send(
                                f"MoFA session `{s.id}` complete.\n"
                                f"Rendered video: {url}"
                            )
                    except Exception:  # noqa: BLE001
                        logger.exception("Failed to send completion message")
                    return

                await asyncio.sleep(poll_interval)

        asyncio.create_task(watch_render_and_notify(session))

        await interaction.followup.send(
            "Setup complete. DaVinci Resolve should open (or already be open).\n"
            "In Resolve, go to `Workspace → Scripts → MoFA_Render` to start the render.\n"
            "Once the MP4 file is created, the bot will be able to share a link.",
            ephemeral=True,
        )

    return bot


def _start_http_server(output_dir: Path) -> None:
    """
    Start a simple HTTP server in a background thread serving the MoFA output directory.
    """
    http_settings = load_http_settings()

    class Handler(SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            # Python 3.7+ allows specifying the directory keyword argument.
            super().__init__(*args, directory=str(output_dir), **kwargs)

    server = ThreadingHTTPServer((http_settings.host, http_settings.port), Handler)
    logger.info(
        "Starting HTTP server on http://%s:%s/ serving %s",
        http_settings.host,
        http_settings.port,
        output_dir,
    )

    loop = asyncio.get_event_loop()
    loop.run_in_executor(None, server.serve_forever)


async def _run_bot_async() -> None:
    _configure_logging()
    if OUTPUT_DIR:
        _start_http_server(Path(OUTPUT_DIR))  # type: ignore[arg-type]
    settings = load_discord_settings()
    bot = create_bot()
    await bot.start(settings.token)


def main() -> None:
    """
    Entry point to launch the MoFA Discord bot.

    Usage (from repository root):
        python -m discord_bot.bot
    """
    try:
        asyncio.run(_run_bot_async())
    except KeyboardInterrupt:
        logger.info("Shutting down bot (KeyboardInterrupt).")


if __name__ == "__main__":
    main()


