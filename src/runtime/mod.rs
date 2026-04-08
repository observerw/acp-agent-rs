//! Runtime helpers that the CLI and transports share when launching ACP agents.
//!
//! This module wires together preparation (downloading binaries, building
//! command specs, managing persistent cached binary payloads) with the
//! subprocess primitives and transport implementations that surface an agent's
//! stdio over different network protocols.
mod cache;
/// Shared archive download/extraction helpers for binary distributions.
pub mod distribution;
/// Command construction and distribution-resolution helpers.
pub mod prepare;
/// Low-level child-process spawning and shutdown helpers.
pub mod process;
/// Shared serve entrypoint and transport selection types.
pub mod serve;
/// Direct stdio execution without a network transport.
pub mod stdio;
/// Concrete TCP, Unix domain socket, HTTP/2, and WebSocket transport implementations.
pub mod transports;
