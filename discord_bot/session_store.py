import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Dict, Optional


@dataclass
class Session:
    """
    Minimal in-memory representation of a MoFA Discord session.

    This can be extended later if we add more complex workflows.
    """

    id: str
    user_id: int
    guild_id: Optional[int]
    channel_id: int
    comp_path: str
    created_at: float = field(default_factory=time.time)
    element_overrides: Dict[int, Any] = field(default_factory=dict)
    font_name: Optional[str] = None
    expected_output_mp4: Optional[str] = None
    status: str = "collecting"  # collecting -> processing -> waiting_render -> done -> error


class SessionStore:
    """
    Simple in-memory session store keyed by session id.
    """

    def __init__(self) -> None:
        self._sessions: Dict[str, Session] = {}

    def create(
        self,
        user_id: int,
        guild_id: Optional[int],
        channel_id: int,
        comp_path: str,
    ) -> Session:
        sid = uuid.uuid4().hex
        session = Session(
            id=sid,
            user_id=user_id,
            guild_id=guild_id,
            channel_id=channel_id,
            comp_path=comp_path,
        )
        self._sessions[sid] = session
        return session

    def get(self, session_id: str) -> Optional[Session]:
        return self._sessions.get(session_id)

    def update(self, session: Session) -> None:
        self._sessions[session.id] = session

    def delete(self, session_id: str) -> None:
        self._sessions.pop(session_id, None)


GLOBAL_SESSION_STORE = SessionStore()

