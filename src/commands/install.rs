use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use tokio::process::Command;

use crate::registry::{
    BinaryTarget, NpxDistribution, Platform, Registry, RegistryAgent, UvxDistribution,
    fetch_registry,
};
use crate::runtime::distribution::prepare_binary_target;

/// Installs an agent by ID using the configured registry distribution.
///
/// The function mirrors the CLI `install` subcommand and returns a
/// descriptive `InstallOutcome` so callers can log where the agent ended up.
pub async fn install_agent(agent_id: &str) -> Result<InstallOutcome> {
    let registry = fetch_registry().await?;
    let agent = registry.get_agent(agent_id)?;

    install_from_registry(&registry, agent).await
}

/// Core installer that inspects each distribution in priority order.
///
/// Binary archives are prepared inside the local `acp-agent` cache when a
/// platform-matching release exists; otherwise the function falls back to npm or
/// uv package installers depending on what the registry exposes.
pub async fn install_from_registry(
    _registry: &Registry,
    agent: &RegistryAgent,
) -> Result<InstallOutcome> {
    if let Some(binary) = &agent.distribution.binary {
        let platform = Platform::current()?;
        if let Some(target) = binary.for_platform(platform) {
            return install_binary(agent, target).await;
        }
    }

    if let Some(npx) = &agent.distribution.npx {
        return install_npx(agent, npx).await;
    }

    if let Some(uvx) = &agent.distribution.uvx {
        return install_uvx(agent, uvx).await;
    }

    Err(anyhow!(
        "agent \"{}\" does not have an installable distribution",
        agent.id
    ))
}

async fn install_npx(
    agent: &RegistryAgent,
    distribution: &NpxDistribution,
) -> Result<InstallOutcome> {
    run_command(
        "npm",
        ["i", "-g", distribution.package.as_str()],
        &format!("npm package {}", distribution.package),
    )
    .await?;

    Ok(InstallOutcome::PackageManager {
        agent_id: agent.id.clone(),
        method: InstallMethod::Npx,
        package: distribution.package.clone(),
    })
}

async fn install_uvx(
    agent: &RegistryAgent,
    distribution: &UvxDistribution,
) -> Result<InstallOutcome> {
    run_command(
        "uv",
        ["tool", "install", distribution.package.as_str()],
        &format!("uv package {}", distribution.package),
    )
    .await?;

    Ok(InstallOutcome::PackageManager {
        agent_id: agent.id.clone(),
        method: InstallMethod::Uvx,
        package: distribution.package.clone(),
    })
}

async fn install_binary(agent: &RegistryAgent, target: &BinaryTarget) -> Result<InstallOutcome> {
    let platform = Platform::current()?;
    let prepared_binary = prepare_binary_target(agent, platform, target).await?;

    Ok(InstallOutcome::Binary {
        agent_id: agent.id.clone(),
        cache_dir: prepared_binary.cache_dir,
    })
}

async fn run_command<I, S>(program: &str, args: I, subject: &str) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args_vec: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let output = Command::new(program)
        .args(&args_vec)
        .output()
        .await
        .with_context(|| format!("failed to run {program}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };

    if detail.is_empty() {
        bail!("failed to run {program} for {subject}");
    }

    bail!("failed to run {program} for {subject}: {detail}");
}

/// Identifier for how an agent was installed when the CLI reports success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    /// The registry provided a ready-to-run binary archive.
    Binary,
    /// The registry points to an npm package invoking `npx`.
    Npx,
    /// The registry points to a uvx package invoking `uv`.
    Uvx,
}

/// Outcome data that is printed by the `install` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// A binary archive was prepared inside the local acp-agent cache.
    Binary {
        /// ID of the agent that was installed.
        agent_id: String,
        /// Filesystem path of the prepared cache directory.
        cache_dir: PathBuf,
    },
    /// A package manager (npm or uv) installed a wrapper on behalf of the agent.
    PackageManager {
        /// ID of the agent that was installed.
        agent_id: String,
        /// Which package-manager strategy was used.
        method: InstallMethod,
        /// Package identifier handed to the installer.
        package: String,
    },
}

impl std::fmt::Display for InstallOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Binary {
                agent_id,
                cache_dir,
            } => {
                write!(f, "Prepared {agent_id} in cache: {}", cache_dir.display())
            }
            Self::PackageManager {
                agent_id,
                method,
                package,
            } => {
                let installer = match method {
                    InstallMethod::Binary => "binary",
                    InstallMethod::Npx => "npm",
                    InstallMethod::Uvx => "uv",
                };
                write!(f, "Installed {agent_id} via {installer}: {package}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::AgentDistribution;

    fn sample_agent() -> RegistryAgent {
        RegistryAgent {
            id: "demo".to_string(),
            name: "Demo".to_string(),
            version: "1.0.0".to_string(),
            description: "Demo agent".to_string(),
            repository: None,
            website: None,
            authors: vec!["ACP".to_string()],
            license: "MIT".to_string(),
            icon: None,
            distribution: AgentDistribution {
                binary: None,
                npx: None,
                uvx: None,
            },
        }
    }

    #[tokio::test]
    async fn reports_missing_distribution() {
        let agent = sample_agent();

        let error = install_from_registry(
            &Registry {
                version: "1".to_string(),
                agents: vec![],
                extensions: None,
            },
            &agent,
        )
        .await
        .expect_err("install should fail");

        assert_eq!(
            error.to_string(),
            "agent \"demo\" does not have an installable distribution"
        );
    }

    #[test]
    fn displays_binary_cache_outcome() {
        let outcome = InstallOutcome::Binary {
            agent_id: "demo".to_string(),
            cache_dir: PathBuf::from("/tmp/acp-agent/demo"),
        };

        assert_eq!(
            outcome.to_string(),
            "Prepared demo in cache: /tmp/acp-agent/demo"
        );
    }
}
