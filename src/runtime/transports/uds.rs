//! Unix domain socket transport that proxies stdin/stdout bytes without framing.
//! Only a single client is accepted so transports that need multiplexing should
//! use the WebSocket or HTTP/2 implementations instead.
#![cfg(unix)]

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitStatus;
#[cfg(test)]
use std::{ffi::OsString, fs};

use anyhow::{Context, Result, bail};
use tokio::net::{UnixListener, UnixStream};

use crate::runtime::prepare::{CommandSpec, PreparedCommand};
use crate::runtime::transports::raw::serve_raw_stream_connection;

/// Accepts one Unix-domain-socket client and shuffles raw bytes between it and the agent.
pub async fn serve_uds(
    prepared: PreparedCommand,
    subject: &str,
    path: &Path,
) -> Result<ExitStatus> {
    if path.exists() {
        bail!(
            "refusing to bind Unix socket at {}: path already exists",
            path.display()
        );
    }

    let mut socket_path_guard = SocketPathGuard::new(path.to_path_buf());
    let listener = UnixListener::bind(path)
        .with_context(|| format!("failed to bind Unix socket listener at {}", path.display()))?;
    socket_path_guard.mark_created();
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

struct SocketPathGuard {
    path: PathBuf,
    created: bool,
}

impl SocketPathGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            created: false,
        }
    }

    fn mark_created(&mut self) {
        self.created = true;
    }
}

impl Drop for SocketPathGuard {
    fn drop(&mut self) {
        if !self.created {
            return;
        }

        let _ = std::fs::remove_file(&self.path);
    }
}

async fn serve_uds_connection(
    spec: CommandSpec,
    subject: &str,
    socket: UnixStream,
) -> Result<ExitStatus> {
    serve_raw_stream_connection(spec, subject, socket).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn prepared_command(program: &str, args: &[&str]) -> PreparedCommand {
        PreparedCommand {
            spec: CommandSpec {
                program: OsString::from(program),
                args: args.iter().map(|arg| OsString::from(*arg)).collect(),
                env: Vec::new(),
                current_dir: None,
            },
            temp_dir: None,
        }
    }

    async fn timeout<F>(future: F) -> F::Output
    where
        F: std::future::Future,
    {
        tokio::time::timeout(std::time::Duration::from_secs(2), future)
            .await
            .expect("timed out waiting for UDS transport test operation")
    }

    #[tokio::test]
    async fn uds_transport_streams_raw_stdio_over_socket() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("agent.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            serve_uds_connection(
                prepared_command("sh", &["-c", "printf 'boot\\n'; cat"]).spec,
                "demo-agent",
                socket,
            )
            .await
        });

        let mut client = UnixStream::connect(&socket_path).await.unwrap();
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

    #[tokio::test]
    async fn uds_bind_fails_when_socket_path_exists() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("agent.sock");
        fs::write(&socket_path, b"occupied").unwrap();

        let error = serve_uds(
            prepared_command("sh", &["-c", "cat"]),
            "demo-agent",
            &socket_path,
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("refusing to bind Unix socket at")
        );
    }

    #[tokio::test]
    async fn uds_bind_does_not_remove_preexisting_path() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("agent.sock");
        fs::write(&socket_path, b"occupied").unwrap();

        let _ = serve_uds(
            prepared_command("sh", &["-c", "cat"]),
            "demo-agent",
            &socket_path,
        )
        .await
        .unwrap_err();

        assert!(socket_path.exists());
        assert_eq!(fs::read(&socket_path).unwrap(), b"occupied");
    }

    #[tokio::test]
    async fn uds_socket_file_is_removed_after_clean_shutdown() {
        let temp_dir = tempdir().unwrap();
        let socket_path = temp_dir.path().join("agent.sock");
        let server_socket_path = socket_path.clone();

        let server = tokio::spawn(async move {
            serve_uds(
                prepared_command("sh", &["-c", "printf 'boot\\n'; cat"]),
                "demo-agent",
                &server_socket_path,
            )
            .await
        });

        loop {
            if socket_path.exists() {
                break;
            }
            tokio::task::yield_now().await;
        }

        let mut client = UnixStream::connect(&socket_path).await.unwrap();
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
        assert!(!socket_path.exists());
    }
}
