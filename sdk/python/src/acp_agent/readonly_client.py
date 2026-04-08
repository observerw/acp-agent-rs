from __future__ import annotations

import asyncio
import inspect
from collections.abc import Awaitable, Callable
from typing import Any

import acp

SessionUpdateCallback = Callable[[acp.SessionNotification], Awaitable[None] | None]


class ReadonlyClient:
    """Client implementation that only consumes session updates."""

    def __init__(
        self,
        on_session_update: SessionUpdateCallback | None = None,
        *,
        max_queue_size: int = 0,
    ) -> None:
        self._on_session_update = on_session_update
        self.session_updates: asyncio.Queue[acp.SessionNotification] = asyncio.Queue(
            maxsize=max_queue_size
        )

    async def request_permission(
        self,
        options: list[Any],
        session_id: str,
        tool_call: Any,
        **kwargs: Any,
    ) -> acp.RequestPermissionResponse:
        del options, session_id, tool_call, kwargs
        return acp.RequestPermissionResponse.model_validate(
            {"outcome": {"outcome": "cancelled"}}
        )

    async def session_update(
        self,
        session_id: str,
        update: Any,
        **kwargs: Any,
    ) -> None:
        payload: dict[str, Any] = {"sessionId": session_id, "update": update}
        if kwargs:
            payload["_meta"] = kwargs

        notification = acp.SessionNotification.model_validate(payload)
        await self.session_updates.put(notification)

        if self._on_session_update is None:
            return
        callback_result = self._on_session_update(notification)
        if inspect.isawaitable(callback_result):
            await callback_result

    async def next_session_update(self) -> acp.SessionNotification:
        return await self.session_updates.get()
