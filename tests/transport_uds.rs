#![cfg(unix)]

mod support;

use std::ffi::OsString;

use acp_agent::runtime::transports::uds::serve_uds_connection;
use anyhow::Result;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use support::transport::{prepared_command_with_program, timeout};

#[tokio::test]
async fn uds_transport_streams_raw_stdio_over_socket() {
    let temp_dir = tempdir().unwrap();
    let socket_path = temp_dir.path().join("agent.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        serve_uds_connection(
            prepared_command_with_program(
                OsString::from("sh"),
                vec![
                    OsString::from("-c"),
                    OsString::from("printf 'boot\\n'; cat"),
                ],
            )
            .spec,
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
