//! TCP transport that proxies stdin/stdout bytes without additional framing.
//! Only a single client is accepted so transports that need multiplexing should
//! use the WebSocket or HTTP/2 implementations instead.
use std::process::ExitStatus;

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};

use crate::runtime::prepare::{CommandSpec, PreparedCommand};
use crate::runtime::transports::raw::serve_raw_stream_connection;

/// Accepts one TCP client and shuffles raw bytes between the socket and the agent.
///
/// The transport keeps running until either the socket closes or the child process
/// exits, and it presents any IO errors via the `Result`.
pub async fn serve_tcp(
    prepared: PreparedCommand,
    subject: &str,
    host: &str,
    port: u16,
) -> Result<ExitStatus> {
    let bound_listener = bind_listener(host, port).await?;
    eprintln!(
        "Running {} over tcp://{} (raw stdio stream transport)",
        subject, bound_listener.display_address
    );

    let (socket, peer_address) = bound_listener.listener.accept().await.with_context(|| {
        format!(
            "failed to accept TCP connection on {}",
            bound_listener.display_address
        )
    })?;
    eprintln!("Accepted connection from {}", peer_address);

    let _temp_dir = prepared.temp_dir;
    serve_tcp_connection(prepared.spec, subject, socket).await
}

#[doc(hidden)]
pub async fn serve_tcp_connection(
    spec: CommandSpec,
    subject: &str,
    socket: TcpStream,
) -> Result<ExitStatus> {
    serve_raw_stream_connection(spec, subject, socket).await
}

struct BoundListener {
    listener: TcpListener,
    display_address: String,
}

async fn bind_listener(host: &str, port: u16) -> Result<BoundListener> {
    let listener = TcpListener::bind((host, port)).await.with_context(|| {
        format!(
            "failed to bind TCP listener on {}",
            bind_target_display(host, port)
        )
    })?;
    let display_address = listener
        .local_addr()
        .map(|address| address.to_string())
        .unwrap_or_else(|_| bind_target_display(host, port));

    Ok(BoundListener {
        listener,
        display_address,
    })
}

fn bind_target_display(host: &str, port: u16) -> String {
    format!("host={host}, port={port}")
}
