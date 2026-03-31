# UDS Support Design

## Goal

Add Unix Domain Socket (UDS) support as a first-class serve mode.

The design should model the concrete serve modes supported by the product and keep the CLI, runtime types, and transport implementation aligned around those modes.

## Public Model

Represent each supported serve mode as a complete variant:

```rust
pub enum ServeMode {
    Http { host: String, port: u16 },
    Tcp { host: String, port: u16 },
    Ws { host: String, port: u16 },
    #[cfg(unix)]
    Uds { path: PathBuf },
}

pub struct ServeOptions {
    pub mode: ServeMode,
}
```

`ServeMode` is the single runtime selector for how an agent is exposed. Each variant fully describes one serving behavior, including the address form it needs.

## Scope

Add exactly one new serving mode:

- `Uds { path }`: raw stdin/stdout byte stream over a Unix domain socket

Existing modes remain unchanged:

- `Http { host, port }`
- `Tcp { host, port }`
- `Ws { host, port }`

## CLI Design

Expose UDS as an explicit transport name:

```bash
acp-agent serve example-agent --transport uds --unix-socket /tmp/acp-agent.sock
```

Rules:

- `--transport uds` requires `--unix-socket <path>`
- `--transport uds` is incompatible with `--host` and `--port`
- `--transport http|tcp|ws` use `--host` and `--port` exactly as they do today
- on non-Unix platforms, `uds` and `--unix-socket` are conditionally compiled out

The CLI should parse directly into `ServeMode`.

## Runtime Structure

The runtime should separate binding from per-connection serving.

Use two layers internally:

1. binding and accept
   - bind TCP listener and accept `TcpStream`
   - bind Unix listener and accept `UnixStream`

2. per-connection serving
   - raw stream connection handler
   - WebSocket connection handler
   - HTTP/2 connection handler

## Raw Stream Refactor

Extract the raw stream connection handler so it can operate on either `TcpStream` or `UnixStream`.

Direction:

```rust
async fn serve_raw_stream_connection<S>(
    spec: CommandSpec,
    subject: &str,
    stream: S,
) -> Result<ExitStatus>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static
```

That shared function should own:

- spawning the child
- pumping socket bytes into child stdin
- pumping child stdout back to the socket
- shutdown and child termination behavior

TCP mode binds `TcpListener`, accepts one client, and calls the shared raw handler.

UDS mode binds `UnixListener`, accepts one client, and calls the same shared raw handler.

`ws.rs` and `h2.rs` should not be generalized as part of this change.

## Socket Lifecycle

UDS needs explicit filesystem rules.

Required behavior:

- if the target socket path already exists, binding fails explicitly
- the implementation must not delete a pre-existing path
- once this process successfully creates the socket file, that file becomes this process's cleanup responsibility
- cleanup must run on all normal return paths, including error returns after a successful bind
- abnormal termination is best-effort only

The cleanup mechanism should be ownership-based, not ad hoc. A small guard type is preferred so the unlink logic is tied to successful creation of the socket path and cannot be accidentally skipped on one return path.

## Testing Plan

Add Unix-only tests for both CLI parsing and transport behavior.

Required cases:

- `parses_serve_subcommand_with_uds_transport`
- `rejects_uds_with_host_or_port`
- `rejects_missing_unix_socket_for_uds`
- `uds_transport_streams_raw_stdio_over_socket`
- `uds_bind_fails_when_socket_path_exists`
- `uds_bind_does_not_remove_preexisting_path`
- `uds_socket_file_is_removed_after_clean_shutdown`

The UDS transport test should follow the structure of [tests/transport_tcp.rs](/Users/wangbowei/workspace/acp-agent/tests/transport_tcp.rs), but use `UnixListener` and `UnixStream`.

## Implementation Order

1. Replace `ServeTransport` in the serve path with a concrete `ServeMode`.
2. Update CLI parsing so `--transport uds` requires `--unix-socket`.
3. Extract a shared raw stream connection handler from the current TCP transport implementation.
4. Implement UDS bind/accept on Unix using that shared raw handler.
5. Add socket lifecycle guard and cleanup behavior.
6. Add Unix-only CLI and integration tests.

## Non-Goals

This proposal intentionally does not cover:

- HTTP/2 over UDS
- WebSocket over UDS
- multi-client UDS serving
- compatibility shims that reinterpret CLI input
