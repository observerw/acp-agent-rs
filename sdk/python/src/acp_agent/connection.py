from __future__ import annotations

import asyncio
import contextlib
import json
import socket
import sys
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from typing import Any

import acp

from .transport import ClientTransport

_BRIDGE_STREAM_LIMIT_BYTES = 4 * 1024 * 1024


class BridgeProtocolError(ValueError):
    """Raised when the bridge sees invalid NDJSON/JSON-RPC messages."""


@asynccontextmanager
async def open_client_connection(
    *,
    client: acp.Client,
    transport: ClientTransport,
) -> AsyncIterator[acp.ClientSideConnection]:
    """Open an ACP client connection over an arbitrary message transport."""

    conn_reader, conn_writer, bridge_reader, bridge_writer = await _create_stream_pair()
    connection = acp.connect_to_agent(client, conn_writer, conn_reader)

    pump_tasks = [
        asyncio.create_task(
            _pump_connection_to_transport(bridge_reader, transport),
            name="acp-agent.bridge.connection_to_transport",
        ),
        asyncio.create_task(
            _pump_transport_to_connection(transport, bridge_writer),
            name="acp-agent.bridge.transport_to_connection",
        ),
    ]

    bridge_error: BaseException | None = None
    try:
        yield connection
    finally:
        for task in pump_tasks:
            task.cancel()
        results = await asyncio.gather(*pump_tasks, return_exceptions=True)
        for result in results:
            if isinstance(result, asyncio.CancelledError):
                continue
            if isinstance(result, BaseException):
                bridge_error = result
                break

        # Close order: bridge tasks -> ACP connection -> transport.
        await connection.close()
        await _close_writer(bridge_writer)
        await _close_writer(conn_writer)
        await transport.close()

        if bridge_error is not None and sys.exc_info()[1] is None:
            raise bridge_error


async def _pump_connection_to_transport(
    bridge_reader: asyncio.StreamReader,
    transport: ClientTransport,
) -> None:
    while True:
        line = await bridge_reader.readline()
        if not line:
            return
        message = _parse_message_line(line)
        await transport.send_message(message)


async def _pump_transport_to_connection(
    transport: ClientTransport,
    bridge_writer: asyncio.StreamWriter,
) -> None:
    while True:
        message = await transport.recv_message()
        bridge_writer.write(_encode_message_line(message))
        await bridge_writer.drain()


def _parse_message_line(line: bytes) -> dict[str, Any]:
    try:
        text = line.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise BridgeProtocolError("Bridge received non-UTF-8 bytes from NDJSON stream") from exc

    if not text.strip():
        raise BridgeProtocolError("Bridge received empty NDJSON line")

    try:
        message = json.loads(text)
    except json.JSONDecodeError as exc:
        raise BridgeProtocolError("Bridge received invalid JSON line") from exc

    if not isinstance(message, dict):
        raise BridgeProtocolError("Bridge JSON line must decode to an object")
    return message


def _encode_message_line(message: dict[str, Any]) -> bytes:
    if not isinstance(message, dict):
        raise TypeError("Bridge can only encode dict messages")
    return (json.dumps(message) + "\n").encode("utf-8")


async def _create_stream_pair() -> tuple[
    asyncio.StreamReader,
    asyncio.StreamWriter,
    asyncio.StreamReader,
    asyncio.StreamWriter,
]:
    sock_a, sock_b = socket.socketpair()
    sock_a.setblocking(False)
    sock_b.setblocking(False)

    try:
        conn_reader, conn_writer = await asyncio.open_connection(
            sock=sock_a,
            limit=_BRIDGE_STREAM_LIMIT_BYTES,
        )
        bridge_reader, bridge_writer = await asyncio.open_connection(
            sock=sock_b,
            limit=_BRIDGE_STREAM_LIMIT_BYTES,
        )
    except Exception:
        sock_a.close()
        sock_b.close()
        raise

    return conn_reader, conn_writer, bridge_reader, bridge_writer


async def _close_writer(writer: asyncio.StreamWriter) -> None:
    writer.close()
    with contextlib.suppress(Exception):
        await writer.wait_closed()
