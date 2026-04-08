from __future__ import annotations

import asyncio
import json
from typing import Any

import acp
import pytest
import websockets
from acp.client.connection import ClientSideConnection

from acp_agent import ReadonlyClient, TransportProtocolError, WebSocketTransport, open_client_connection


class _IdleTransport:
    def __init__(self) -> None:
        self.closed = False
        self.sent_messages: list[dict[str, Any]] = []
        self._recv_waiter = asyncio.Event()

    async def send_message(self, message: dict[str, Any]) -> None:
        self.sent_messages.append(message)

    async def recv_message(self) -> dict[str, Any]:
        await self._recv_waiter.wait()
        raise EOFError("idle transport closed")

    async def close(self) -> None:
        self.closed = True
        self._recv_waiter.set()


def test_websocket_transport_round_trip() -> None:
    async def _scenario() -> None:
        server_done = asyncio.Event()

        async def handler(ws: websockets.ServerConnection) -> None:
            raw = await ws.recv()
            assert isinstance(raw, str)
            payload = json.loads(raw)
            assert payload["method"] == "ping"
            await ws.send(raw)
            await ws.close()
            server_done.set()

        async with websockets.serve(handler, "127.0.0.1", 0) as server:
            host, port = server.sockets[0].getsockname()[:2]
            transport = await WebSocketTransport.connect(f"ws://{host}:{port}")
            await transport.send_message({"jsonrpc": "2.0", "method": "ping"})
            echoed = await transport.recv_message()
            assert echoed["method"] == "ping"
            await transport.close()
            await asyncio.wait_for(server_done.wait(), timeout=1)

    asyncio.run(_scenario())


@pytest.mark.parametrize(
    ("payload", "message"),
    [
        (b"binary", "binary frame"),
        ("not-json", "valid JSON"),
        ("[]", "must be an object"),
    ],
)
def test_websocket_transport_rejects_invalid_frames(payload: Any, message: str) -> None:
    async def _scenario() -> None:
        async def handler(ws: websockets.ServerConnection) -> None:
            await ws.send(payload)
            await ws.close()

        async with websockets.serve(handler, "127.0.0.1", 0) as server:
            host, port = server.sockets[0].getsockname()[:2]
            transport = await WebSocketTransport.connect(f"ws://{host}:{port}")
            with pytest.raises(TransportProtocolError, match=message):
                await transport.recv_message()
            await transport.close()

    asyncio.run(_scenario())


def test_open_client_connection_bridges_initialize_and_updates() -> None:
    async def _scenario() -> None:
        response_queue: asyncio.Queue[dict[str, Any]] = asyncio.Queue()
        callback_updates: list[acp.SessionNotification] = []

        async def handler(ws: websockets.ServerConnection) -> None:
            init_req_raw = await ws.recv()
            assert isinstance(init_req_raw, str)
            init_req = json.loads(init_req_raw)
            assert init_req["method"] == "initialize"

            await ws.send(
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": init_req["id"],
                        "result": {
                            "protocolVersion": 1,
                            "agentCapabilities": {},
                            "authMethods": [],
                        },
                    }
                )
            )
            await ws.send(
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": "session-1",
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": {"type": "text", "text": "hello"},
                            },
                        },
                    }
                )
            )
            await ws.send(
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": 900,
                        "method": "fs/read_text_file",
                        "params": {"sessionId": "session-1", "path": "/tmp/demo.txt"},
                    }
                )
            )

            reply_raw = await ws.recv()
            assert isinstance(reply_raw, str)
            await response_queue.put(json.loads(reply_raw))
            await ws.close()

        async with websockets.serve(handler, "127.0.0.1", 0) as server:
            host, port = server.sockets[0].getsockname()[:2]
            transport = await WebSocketTransport.connect(f"ws://{host}:{port}")
            client = ReadonlyClient(on_session_update=callback_updates.append)

            async with open_client_connection(client=client, transport=transport) as conn:
                assert isinstance(conn, ClientSideConnection)
                init_res = await conn.initialize(protocol_version=1)
                assert init_res.protocol_version == 1
                queued_update = await asyncio.wait_for(
                    client.next_session_update(), timeout=1
                )
                assert queued_update.session_id == "session-1"
                server_response = await asyncio.wait_for(response_queue.get(), timeout=1)

            assert server_response["id"] == 900
            assert server_response["error"]["code"] == -32601
            assert callback_updates and callback_updates[0].session_id == "session-1"

    asyncio.run(_scenario())


def test_readonly_client_request_permission_returns_cancelled() -> None:
    async def _scenario() -> None:
        client = ReadonlyClient()
        response = await client.request_permission([], "session-1", object())
        assert response.outcome.outcome == "cancelled"

    asyncio.run(_scenario())


def test_open_client_connection_closes_transport_on_exit() -> None:
    async def _scenario() -> None:
        transport = _IdleTransport()
        client = ReadonlyClient()
        async with open_client_connection(client=client, transport=transport) as conn:
            assert isinstance(conn, ClientSideConnection)
        assert transport.closed

    asyncio.run(_scenario())
