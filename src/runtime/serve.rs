use anyhow::{Context, Result};
#[cfg(unix)]
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::registry::fetch_registry;
use crate::runtime::prepare::prepare_agent_command;
#[cfg(unix)]
use crate::runtime::transports::uds;
use crate::runtime::transports::{h2, tcp, ws};

/// Concrete network serve modes supported by the runtime.
///
/// Each variant fully describes how the agent should be exposed, including the
/// listener addressing details needed by that transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeMode {
    /// HTTP/2 stream transport implemented by [`crate::runtime::transports::h2`].
    Http {
        /// Listener host or IP address to bind.
        host: String,
        /// Listener port. `0` lets the OS choose an ephemeral port.
        port: u16,
    },
    /// Raw TCP byte-stream transport implemented by [`crate::runtime::transports::tcp`].
    Tcp {
        /// Listener host or IP address to bind.
        host: String,
        /// Listener port. `0` lets the OS choose an ephemeral port.
        port: u16,
    },
    /// WebSocket transport: one ACP message per WebSocket frame, implemented by [`crate::runtime::transports::ws`].
    Ws {
        /// Listener host or IP address to bind.
        host: String,
        /// Listener port. `0` lets the OS choose an ephemeral port.
        port: u16,
    },
    /// Unix domain socket raw byte-stream transport.
    #[cfg(unix)]
    Uds {
        /// Filesystem path where the Unix domain socket listener will bind.
        path: PathBuf,
    },
}

/// Runtime options shared by all serve entrypoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOptions {
    /// Complete serve behavior, including transport selection and bind target.
    pub mode: ServeMode,
}

/// Serves an ACP agent via the chosen transport so external clients can interact.
///
/// Fetches the registry entry, prepares the agent command (downloading binaries if
/// needed), and then dispatches to the transport module that knows how to wire
/// stdio across TCP, HTTP/2, or WebSocket.
pub async fn serve_agent(
    agent_id: &str,
    options: ServeOptions,
    user_args: &[String],
) -> Result<ExitStatus> {
    let registry = fetch_registry().await?;
    let agent = registry
        .get_agent(agent_id)
        .with_context(|| format!("failed to resolve agent \"{agent_id}\" from registry"))?;

    let prepared = prepare_agent_command(agent, user_args).await?;

    match options.mode {
        ServeMode::Http { host, port } => h2::serve_h2(prepared, &agent.id, &host, port).await,
        ServeMode::Tcp { host, port } => tcp::serve_tcp(prepared, &agent.id, &host, port).await,
        ServeMode::Ws { host, port } => ws::serve_ws(prepared, &agent.id, &host, port).await,
        #[cfg(unix)]
        ServeMode::Uds { path } => uds::serve_uds(prepared, &agent.id, &path).await,
    }
}
