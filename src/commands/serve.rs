use std::process::ExitStatus;

use anyhow::Result;

use crate::runtime::serve::{ServeOptions, ServeTransport, serve_agent as runtime_serve_agent};

pub async fn serve_agent(
    agent_id: &str,
    transport: ServeTransport,
    host: String,
    port: u16,
    args: &[String],
) -> Result<ExitStatus> {
    runtime_serve_agent(
        agent_id,
        ServeOptions {
            transport,
            host,
            port,
        },
        args,
    )
    .await
}
