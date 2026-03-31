//! Shared raw byte-stream connection handler for single-socket transports.
//!
//! This module owns the child-process lifecycle and the bidirectional copying
//! between an arbitrary async stream and the child's stdin/stdout pipes.
use std::convert::Infallible;
use std::future::pending;
use std::process::ExitStatus;

use anyhow::{Context, Result, anyhow};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf, copy, split};
use tokio::process::{ChildStdin, ChildStdout};

use crate::runtime::prepare::CommandSpec;
use crate::runtime::process::{spawn_stream_child, terminate_child};

/// Serves one raw byte-stream connection by proxying it to the agent stdio.
pub async fn serve_raw_stream_connection<S>(
    spec: CommandSpec,
    subject: &str,
    stream: S,
) -> Result<ExitStatus>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut child = spawn_stream_child(&spec, subject)?;
    let child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("child process missing piped stdin"))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child process missing piped stdout"))?;

    let (stream_reader, stream_writer) = split(stream);
    let stdin_pump = pump_stream_to_child_until_error(stream_reader, child_stdin);
    let stdout_pump = pump_child_to_stream(child_stdout, stream_writer);
    tokio::pin!(stdin_pump);
    tokio::pin!(stdout_pump);

    tokio::select! {
        status = child.wait() => status.context("failed while waiting on child process"),
        result = &mut stdin_pump => {
            match result {
                Ok(never) => match never {},
                Err(source) => {
                    let _ = terminate_child(&mut child).await;
                    Err(anyhow!("raw stream transport failed: {source}"))
                }
            }
        }
        result = &mut stdout_pump => {
            match result {
                Ok(()) => terminate_child(&mut child).await,
                Err(source) => {
                    let _ = terminate_child(&mut child).await;
                    Err(anyhow!("raw stream transport failed: {source}"))
                }
            }
        }
    }
}

async fn pump_stream_to_child_until_error<S>(
    mut stream_reader: ReadHalf<S>,
    mut child_stdin: ChildStdin,
) -> Result<Infallible>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    copy(&mut stream_reader, &mut child_stdin)
        .await
        .context("failed to read from client stream or write to child stdin")?;
    child_stdin
        .shutdown()
        .await
        .context("failed to close child stdin after client EOF")?;
    drop(child_stdin);

    Ok(pending::<Infallible>().await)
}

async fn pump_child_to_stream<S>(
    mut child_stdout: ChildStdout,
    mut stream_writer: WriteHalf<S>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    copy(&mut child_stdout, &mut stream_writer)
        .await
        .context("failed to read from child stdout or write to client stream")?;
    stream_writer
        .shutdown()
        .await
        .context("failed to close client stream after child stdout EOF")?;
    Ok(())
}
