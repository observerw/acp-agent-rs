# UDS Refactor Implementation Plan

## Purpose

This document turns [UDS_DESIGN.md](/Users/wangbowei/workspace/acp-agent/UDS_DESIGN.md) into an implementation plan for the current codebase.

The target is not just to add one more transport flag. The target is to realign the CLI, runtime model, and transport implementations around a single concrete serve-mode model, then add Unix domain socket support on top of that model.

## Current Gaps

The current code does not yet match the design in several important ways:

- [src/runtime/serve.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/serve.rs) models serving as `ServeTransport + host + port`, not as a self-contained `ServeMode`.
- [src/commands/mod.rs](/Users/wangbowei/workspace/acp-agent/src/commands/mod.rs) exposes `transport`, `host`, and `port` directly in the `serve` subcommand, so it cannot express UDS-specific argument rules cleanly.
- [src/commands/serve.rs](/Users/wangbowei/workspace/acp-agent/src/commands/serve.rs) mirrors the same split model and will need to be updated together with the runtime types.
- [src/runtime/transports/tcp.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/transports/tcp.rs) mixes TCP listener binding with the raw byte-stream connection handler, so the stream logic is not reusable for `UnixStream`.
- [tests/transport_tcp.rs](/Users/wangbowei/workspace/acp-agent/tests/transport_tcp.rs) already captures the intended behavior for the raw stdio stream transport and should become the template for UDS integration coverage.

Baseline status at the time this plan was written:

- `cargo test -q` passes.

## Constraints

- Follow [UDS_DESIGN.md](/Users/wangbowei/workspace/acp-agent/UDS_DESIGN.md) exactly for scope and non-goals.
- Do not add compatibility shims that reinterpret invalid CLI input into some other mode.
- Do not generalize WebSocket or HTTP/2 as part of this change.
- Keep non-Unix builds compiling cleanly by gating UDS types and tests with `cfg(unix)`.

## Step 1: Replace Transport-Centric Serve Types With `ServeMode`

### Goal

Make the runtime model represent complete serve behaviors directly.

### Change Scope

- Update [src/runtime/serve.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/serve.rs):
  - Remove `ServeTransport`.
  - Introduce `ServeMode`:
    - `Http { host: String, port: u16 }`
    - `Tcp { host: String, port: u16 }`
    - `Ws { host: String, port: u16 }`
    - `Uds { path: PathBuf }` behind `#[cfg(unix)]`
  - Change `ServeOptions` to:
    - `pub mode: ServeMode`
- Update [src/commands/serve.rs](/Users/wangbowei/workspace/acp-agent/src/commands/serve.rs):
  - Stop accepting separate `transport`, `host`, and `port`.
  - Accept either `ServeOptions` or `ServeMode` directly.
- Update any call sites in [src/commands/mod.rs](/Users/wangbowei/workspace/acp-agent/src/commands/mod.rs) to construct the new serve model before entering the runtime.

### Acceptance Criteria

- Runtime dispatch is `match options.mode`, not `match options.transport`.
- No remaining runtime path depends on a detached `transport + host/port` combination.
- Existing `http`, `tcp`, and `ws` behavior remains unchanged.
- The crate compiles after the type migration.

## Step 2: Redesign CLI Parsing Around Concrete Serve Modes

### Goal

Make the `serve` subcommand parse into the new serve model and enforce the UDS-specific CLI rules described in the design.

### Change Scope

- Update [src/commands/mod.rs](/Users/wangbowei/workspace/acp-agent/src/commands/mod.rs):
  - Add `--unix-socket <path>` on Unix.
  - Allow `--transport uds` on Unix only.
  - Keep `--host` and `--port` for `http`, `tcp`, and `ws`.
  - Ensure `--transport uds`:
    - requires `--unix-socket`
    - rejects `--host`
    - rejects `--port`
- Add a CLI normalization layer that converts parsed arguments into `ServeMode`.
- Update the existing serve parser tests in the same file.

### Acceptance Criteria

- The CLI can represent:
  - `acp-agent serve demo-agent`
  - `acp-agent serve demo-agent --transport tcp`
  - `acp-agent serve demo-agent --transport ws --host 0.0.0.0 --port 8010`
  - `acp-agent serve demo-agent --transport uds --unix-socket /tmp/acp-agent.sock` on Unix
- These tests exist and pass:
  - `parses_serve_subcommand_with_uds_transport`
  - `rejects_uds_with_host_or_port`
  - `rejects_missing_unix_socket_for_uds`
- On non-Unix platforms, `uds` and `--unix-socket` are not compiled into the CLI.

## Step 3: Extract a Shared Raw Stream Connection Handler

### Goal

Separate raw stream connection serving from TCP listener setup so the same connection logic can serve both `TcpStream` and `UnixStream`.

### Change Scope

- Refactor [src/runtime/transports/tcp.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/transports/tcp.rs):
  - Move child process spawning, socket-to-stdin copy, stdout-to-socket copy, shutdown behavior, and child termination behavior into a shared function.
- Add a new shared raw-stream module if needed, for example under:
  - [src/runtime/transports/mod.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/transports/mod.rs)
  - or a new sibling module such as `raw.rs`
- Keep [src/runtime/process.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/process.rs) as the owner of low-level child lifecycle helpers.

### Target Shape

The shared entrypoint should follow the design direction:

```rust
async fn serve_raw_stream_connection<S>(
    spec: CommandSpec,
    subject: &str,
    stream: S,
) -> Result<ExitStatus>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static
```

### Acceptance Criteria

- TCP listener setup remains in the TCP transport module.
- The raw stream connection handler is reusable with both `TcpStream` and `UnixStream`.
- [tests/transport_tcp.rs](/Users/wangbowei/workspace/acp-agent/tests/transport_tcp.rs) still passes without behavior changes.
- No WebSocket or HTTP/2 refactor is introduced in this step.

## Step 4: Add Unix Domain Socket Transport

### Goal

Implement the UDS serve mode by binding a Unix listener, accepting one client, and handing the accepted stream to the shared raw stream handler.

### Change Scope

- Add a Unix-only transport module:
  - [src/runtime/transports/uds.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/transports/uds.rs)
- Update [src/runtime/transports/mod.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/transports/mod.rs) to export `uds` behind `#[cfg(unix)]`.
- Update [src/runtime/serve.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/serve.rs) to dispatch `ServeMode::Uds { path }` to the new module.
- Update public-facing docs if needed in [src/lib.rs](/Users/wangbowei/workspace/acp-agent/src/lib.rs) so serve modes are documented consistently.

### Acceptance Criteria

- On Unix, `ServeMode::Uds { path }` compiles and is routable from the runtime entrypoint.
- The UDS transport accepts a single client connection and proxies raw stdio bytes exactly like TCP mode.
- A Unix-only integration test exists and passes:
  - `uds_transport_streams_raw_stdio_over_socket`

## Step 5: Implement Socket Lifecycle Ownership and Cleanup

### Goal

Make UDS filesystem behavior explicit and safe.

### Change Scope

- Implement the lifecycle logic in [src/runtime/transports/uds.rs](/Users/wangbowei/workspace/acp-agent/src/runtime/transports/uds.rs):
  - Fail if the socket path already exists.
  - Do not delete any pre-existing path.
  - Track ownership of the successfully created socket path.
  - Remove the socket path on all normal return paths after a successful bind.
- Prefer a small guard type whose drop behavior owns the unlink responsibility for paths created by this process.
- Add any reusable temp-path test helpers to [tests/support/transport.rs](/Users/wangbowei/workspace/acp-agent/tests/support/transport.rs) if needed.

### Acceptance Criteria

- If the target socket path already exists, bind fails with a clear error.
- A pre-existing filesystem entry is not removed by the failed bind path.
- A socket file created by this process is removed after clean shutdown.
- These Unix-only tests exist and pass:
  - `uds_bind_fails_when_socket_path_exists`
  - `uds_bind_does_not_remove_preexisting_path`
  - `uds_socket_file_is_removed_after_clean_shutdown`

## Step 6: Finalize Test Matrix and Documentation Consistency

### Goal

Make the feature complete from both correctness and maintainability perspectives.

### Change Scope

- Add or update Unix-only integration coverage in:
  - [tests/transport_uds.rs](/Users/wangbowei/workspace/acp-agent/tests/transport_uds.rs)
- Expand CLI parser coverage in:
  - [src/commands/mod.rs](/Users/wangbowei/workspace/acp-agent/src/commands/mod.rs)
- Update transport-mode documentation where necessary:
  - [src/lib.rs](/Users/wangbowei/workspace/acp-agent/src/lib.rs)
  - optionally project README files if they document `serve`
- Review `cfg(unix)` boundaries to ensure Unix-only types do not leak into non-Unix builds.

### Acceptance Criteria

- `cargo test` passes on Unix with the new UDS coverage included.
- Non-Unix builds still compile without UDS symbols.
- CLI help, runtime types, and transport modules describe the same supported serve modes.
- No undocumented mismatch remains between the public CLI and internal runtime model.

## Suggested Execution Order

1. Replace `ServeTransport` with `ServeMode`.
2. Update CLI parsing and argument validation for `uds`.
3. Extract the shared raw stream connection handler from TCP.
4. Add Unix domain socket bind/accept support on top of the shared raw handler.
5. Add the socket lifecycle guard and cleanup behavior.
6. Finish Unix-only CLI and integration tests and align docs.

## Out of Scope

The following are intentionally not part of this implementation:

- HTTP/2 over UDS
- WebSocket over UDS
- multi-client UDS serving
- compatibility behavior that silently rewrites invalid CLI combinations
