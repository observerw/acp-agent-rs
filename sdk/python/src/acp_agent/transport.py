from __future__ import annotations

import json
from typing import Any, Protocol, cast, runtime_checkable

import websockets
from websockets.asyncio.client import ClientConnection


class TransportProtocolError(ValueError):
    """Raised when a transport frame violates ACP JSON-RPC framing rules."""


@runtime_checkable
class ClientTransport(Protocol):
    """Bidirectional ACP message transport."""

    async def send_message(self, message: dict[str, Any]) -> None:
        """Send one JSON-RPC message."""

    async def recv_message(self) -> dict[str, Any]:
        """Receive one JSON-RPC message."""

    async def close(self) -> None:
        """Close transport resources."""


class WebSocketTransport:
    """WebSocket transport with one ACP message per text frame."""

    def __init__(self, websocket: ClientConnection) -> None:
        self._websocket = websocket

    @classmethod
    async def connect(cls, uri: str, **kwargs: Any) -> WebSocketTransport:
        websocket = await websockets.connect(uri, **kwargs)
        return cls(websocket)

    async def send_message(self, message: dict[str, Any]) -> None:
        if not isinstance(message, dict):
            raise TypeError("message must be a dict")
        await self._websocket.send(json.dumps(message))

    async def recv_message(self) -> dict[str, Any]:
        payload = await self._websocket.recv()
        if isinstance(payload, bytes):
            raise TransportProtocolError(
                "WebSocket transport only accepts text frames; binary frame rejected"
            )
        if not isinstance(payload, str):
            raise TransportProtocolError("WebSocket payload must be UTF-8 text")
        try:
            message = json.loads(payload)
        except json.JSONDecodeError as exc:
            raise TransportProtocolError("WebSocket frame is not valid JSON") from exc
        if not isinstance(message, dict):
            raise TransportProtocolError("WebSocket frame JSON must be an object")
        return cast(dict[str, Any], message)

    async def close(self) -> None:
        await self._websocket.close()
