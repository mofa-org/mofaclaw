import os
from dataclasses import dataclass


@dataclass(frozen=True)
class DiscordSettings:
    token: str
    guild_ids: tuple[int, ...]  # Optional: restrict command registration


@dataclass(frozen=True)
class HttpServerSettings:
    host: str
    port: int


def load_discord_settings() -> DiscordSettings:
    """
    Load Discord-related configuration from environment variables.

    Expected variables:
    - MOFA_DISCORD_TOKEN: bot token (required)
    - MOFA_DISCORD_GUILDS: optional, comma-separated guild IDs for scoped command sync
    """
    token = os.getenv("MOFA_DISCORD_TOKEN", "").strip()
    if not token:
        raise RuntimeError(
            "MOFA_DISCORD_TOKEN is not set. "
            "Set it in your environment before running the bot."
        )

    raw_guilds = os.getenv("MOFA_DISCORD_GUILDS", "").strip()
    guild_ids: tuple[int, ...] = ()
    if raw_guilds:
        parts = [p.strip() for p in raw_guilds.split(",") if p.strip()]
        guild_ids = tuple(int(p) for p in parts if p.isdigit())

    return DiscordSettings(token=token, guild_ids=guild_ids)


def load_http_settings() -> HttpServerSettings:
    """
    Load HTTP server settings.

    Expected variables:
    - MOFA_HTTP_HOST: hostname or IP used in links (default: localhost)
    - MOFA_HTTP_PORT: integer port (default: 8000)
    """
    host = os.getenv("MOFA_HTTP_HOST", "localhost").strip() or "localhost"
    raw_port = os.getenv("MOFA_HTTP_PORT", "8000").strip() or "8000"
    try:
        port = int(raw_port)
    except ValueError:
        port = 8000

    return HttpServerSettings(host=host, port=port)


