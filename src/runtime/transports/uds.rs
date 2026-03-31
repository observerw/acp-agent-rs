//! Unix domain socket transport that proxies stdin/stdout bytes without framing.
//! Only a single client is accepted so transports that need multiplexing should
//! use the WebSocket or HTTP/2 implementations instead.
#![cfg(unix)]

use std::path::Path;
use std::process::ExitStatus;

use anyhow::{Context, Result};
use tokio::net::{UnixListener, UnixStream};

use crate::runtime::prepare::{CommandSpec, PreparedCommand};
use crate::runtime::transports::raw::serve_raw_stream_connection;

/// Accepts one Unix-domain-socket client and shuffles raw bytes between it and the agent.
pub async fn serve_uds(
    prepared: PreparedCommand,
    subject: &str,
    path: &Path,
) -> Result<ExitStatus> {
    let listener = UnixListener::bind(path)
        .with_context(|| format!("failed to bind Unix socket listener at {}", path.display()))?;
    eprintln!(
        "Running {} over unix://{} (raw stdio stream transport)",
        subject,
        path.display()
    );

    let (socket, _peer_address) = listener.accept().await.with_context(|| {
        format!(
            "failed to accept Unix socket connection on {}",
            path.display()
        )
    })?;
    eprintln!("Accepted connection on {}", path.display());

    let _temp_dir = prepared.temp_dir;
    serve_uds_connection(prepared.spec, subject, socket).await
}

#[doc(hidden)]
pub async fn serve_uds_connection(
    spec: CommandSpec,
    subject: &str,
    socket: UnixStream,
) -> Result<ExitStatus> {
    serve_raw_stream_connection(spec, subject, socket).await
}
