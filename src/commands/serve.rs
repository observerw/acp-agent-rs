use std::process::ExitStatus;

use anyhow::Result;

use crate::runtime::serve::{ServeMode, ServeOptions, serve_agent as runtime_serve_agent};

/// Binds an ACP agent to the requested transport/host/port, matching `serve`.
///
/// The function returns the `ExitStatus` of the spawned agent so the caller can
/// surface failures that happen while the agent is running under a network
/// transport.
pub async fn serve_agent(agent_id: &str, mode: ServeMode, args: &[String]) -> Result<ExitStatus> {
    runtime_serve_agent(agent_id, ServeOptions { mode }, args).await
}
