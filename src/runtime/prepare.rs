use std::ffi::OsString;
use std::path::PathBuf;

use crate::registry::{BinaryTarget, Environment, Platform, RegistryAgent};
use crate::runtime::distribution::prepare_binary_target;
use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A concrete instruction set for invoking an agent binary or package runner.
///
/// `program` is the executable path (binary or package manager), `args` are the
/// command line arguments already resolved by the runtime, `env` holds any
/// injected environment overrides, and `current_dir` is the working directory
/// (only set when a binary distribution was unpacked locally).
pub struct CommandSpec {
    /// The executable (or package manager) that will be launched.
    pub program: OsString,
    /// Arguments prepopulated by the runtime before user args are appended.
    pub args: Vec<OsString>,
    /// Environment overrides that should be injected into the child process.
    pub env: Vec<(OsString, OsString)>,
    /// Optional working directory used by the child process (set for binaries).
    pub current_dir: Option<PathBuf>,
}

#[derive(Debug)]
/// The prepared payload returned to transports and runners.
///
/// For binary distributions the resolved executable lives under a persistent
/// cache directory that can be reused across invocations.
pub struct PreparedCommand {
    /// The command specification suitable for `tokio::process::Command`.
    pub spec: CommandSpec,
}

/// Builds a `PreparedCommand` from a registry entry and extra user arguments.
///
/// The runtime first attempts to download and extract a binary distribution for
/// the current platform, then falls back to `npx` or `uvx` package runners if no
/// binary target exists.
pub async fn prepare_agent_command(
    agent: &RegistryAgent,
    user_args: &[String],
) -> Result<PreparedCommand> {
    if let Some(binary) = &agent.distribution.binary {
        let platform = Platform::current()?;
        if let Some(target) = binary.for_platform(platform) {
            let prepared_binary = prepare_binary_target(agent, platform, target).await?;

            return Ok(PreparedCommand {
                spec: binary_command_spec(
                    prepared_binary.executable_path,
                    prepared_binary.extracted_dir,
                    target,
                    user_args,
                ),
            });
        }
    }

    if let Some(npx) = &agent.distribution.npx {
        return Ok(PreparedCommand {
            spec: package_command_spec(
                "npx",
                &npx.package,
                npx.args.as_ref(),
                npx.env.as_ref(),
                user_args,
            ),
        });
    }

    if let Some(uvx) = &agent.distribution.uvx {
        return Ok(PreparedCommand {
            spec: package_command_spec(
                "uvx",
                &uvx.package,
                uvx.args.as_ref(),
                uvx.env.as_ref(),
                user_args,
            ),
        });
    }

    bail!(
        "agent \"{}\" does not have a runnable distribution",
        agent.id
    )
}

fn package_command_spec(
    program: &str,
    package: &str,
    args: Option<&Vec<String>>,
    env: Option<&Environment>,
    user_args: &[String],
) -> CommandSpec {
    let mut command_args = vec![OsString::from(package)];
    if let Some(args) = args {
        command_args.extend(args.iter().cloned().map(OsString::from));
    }
    command_args.extend(user_args.iter().cloned().map(OsString::from));

    CommandSpec {
        program: OsString::from(program),
        args: command_args,
        env: clone_env_pairs(env),
        current_dir: None,
    }
}

fn binary_command_spec(
    executable_path: PathBuf,
    extracted_dir: PathBuf,
    target: &BinaryTarget,
    user_args: &[String],
) -> CommandSpec {
    let mut args: Vec<OsString> = target
        .args
        .as_ref()
        .into_iter()
        .flatten()
        .cloned()
        .map(OsString::from)
        .collect();
    args.extend(user_args.iter().cloned().map(OsString::from));

    CommandSpec {
        program: executable_path.into_os_string(),
        args,
        env: clone_env_pairs(target.env.as_ref()),
        current_dir: Some(extracted_dir),
    }
}

fn clone_env_pairs(env: Option<&Environment>) -> Vec<(OsString, OsString)> {
    env.into_iter()
        .flat_map(|pairs| pairs.iter())
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}
