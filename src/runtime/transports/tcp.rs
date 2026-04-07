//! TCP transport that proxies stdin/stdout bytes without additional framing.
//! Only a single client is accepted so transports that need multiplexing should
//! use the WebSocket or HTTP/2 implementations instead.
use std::process::ExitStatus;

use anyhow::{Context, Result};
#[cfg(test)]
use std::ffi::OsString;
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

    serve_tcp_connection(prepared.spec, subject, socket).await
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

async fn serve_tcp_connection(
    spec: CommandSpec,
    subject: &str,
    socket: TcpStream,
) -> Result<ExitStatus> {
    serve_raw_stream_connection(spec, subject, socket).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn prepared_command(program: &str, args: &[&str]) -> PreparedCommand {
        PreparedCommand {
            spec: CommandSpec {
                program: OsString::from(program),
                args: args.iter().map(|arg| OsString::from(*arg)).collect(),
                env: Vec::new(),
                current_dir: None,
            },
        }
    }

    async fn timeout<F>(future: F) -> F::Output
    where
        F: std::future::Future,
    {
        tokio::time::timeout(std::time::Duration::from_secs(2), future)
            .await
            .expect("timed out waiting for TCP transport test operation")
    }

    #[tokio::test]
    async fn tcp_transport_streams_raw_stdio_over_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener
                .accept()
                .await
                .context("failed to accept TCP client")?;
            serve_tcp_connection(
                prepared_command("sh", &["-c", "printf 'boot\\n'; cat"]).spec,
                "demo-agent",
                socket,
            )
            .await
        });

        let mut client = TcpStream::connect(address).await.unwrap();
        let mut first_chunk = [0_u8; 5];
        timeout(client.read_exact(&mut first_chunk)).await.unwrap();
        assert_eq!(&first_chunk, b"boot\n");

        client.write_all(b"ping\n").await.unwrap();
        client.shutdown().await.unwrap();

        let mut echoed = Vec::new();
        timeout(client.read_to_end(&mut echoed)).await.unwrap();
        assert_eq!(echoed, b"ping\n");

        let status: Result<_> = timeout(server).await.unwrap();
        assert!(status.unwrap().success());
    }
}
