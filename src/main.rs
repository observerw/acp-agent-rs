use std::io::Write;
use std::process::ExitStatus;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "acp-agent",
    version,
    about = "Install, run, and serve ACP agents from the public registry."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install {
        agent_id: String,
    },
    #[command(trailing_var_arg = true)]
    Run {
        agent_id: String,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(trailing_var_arg = true)]
    Serve {
        agent_id: String,
        #[arg(long)]
        port: u16,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliExit {
    Success,
    Code(i32),
}

async fn execute_cli<W: Write>(cli: Cli, writer: &mut W) -> anyhow::Result<CliExit> {
    match cli.command {
        Commands::Install { agent_id } => {
            let outcome = acp_agent::install::install_agent(&agent_id)
                .await
                .with_context(|| format!("failed to install agent \"{agent_id}\""))?;
            writeln!(writer, "{outcome}")?;
            Ok(CliExit::Success)
        }
        Commands::Run { agent_id, args } => {
            let status = acp_agent::run::run_agent(&agent_id, &args)
                .await
                .with_context(|| format!("failed to run agent \"{agent_id}\""))?;
            Ok(exit_from_status(status))
        }
        Commands::Serve {
            agent_id,
            port,
            host,
            args,
        } => {
            let status = acp_agent::serve::serve_agent(&agent_id, &host, port, &args)
                .await
                .with_context(|| format!("failed to serve agent \"{agent_id}\""))?;
            Ok(exit_from_status(status))
        }
    }
}

fn exit_from_status(status: ExitStatus) -> CliExit {
    if status.success() {
        return CliExit::Success;
    }

    if let Some(code) = status.code() {
        return CliExit::Code(code);
    }

    CliExit::Code(signal_exit_code(status))
}

#[cfg(unix)]
fn signal_exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;

    status.signal().map_or(1, |signal| 128 + signal)
}

#[cfg(not(unix))]
fn signal_exit_code(_: ExitStatus) -> i32 {
    1
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    match execute_cli(cli, &mut stdout).await? {
        CliExit::Success => Ok(()),
        CliExit::Code(code) => std::process::exit(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_install_subcommand() {
        let cli = Cli::try_parse_from(["acp-agent", "install", "demo-agent"]).unwrap();

        match cli.command {
            Commands::Install { agent_id } => assert_eq!(agent_id, "demo-agent"),
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn parses_run_subcommand_with_trailing_hyphenated_args() {
        let cli = Cli::try_parse_from([
            "acp-agent",
            "run",
            "demo-agent",
            "--model",
            "gpt-5",
            "--flag",
        ])
        .unwrap();

        match cli.command {
            Commands::Run { agent_id, args } => {
                assert_eq!(agent_id, "demo-agent");
                assert_eq!(args, vec!["--model", "gpt-5", "--flag"]);
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn parses_serve_subcommand_with_default_host_and_trailing_args() {
        let cli = Cli::try_parse_from([
            "acp-agent",
            "serve",
            "demo-agent",
            "--port",
            "8080",
            "--",
            "--model",
            "gpt-5",
        ])
        .unwrap();

        match cli.command {
            Commands::Serve {
                agent_id,
                port,
                host,
                args,
            } => {
                assert_eq!(agent_id, "demo-agent");
                assert_eq!(port, 8080);
                assert_eq!(host, "127.0.0.1");
                assert_eq!(args, vec!["--model", "gpt-5"]);
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn parses_serve_subcommand_with_custom_host() {
        let cli = Cli::try_parse_from([
            "acp-agent",
            "serve",
            "demo-agent",
            "--port",
            "9000",
            "--host",
            "0.0.0.0",
        ])
        .unwrap();

        match cli.command {
            Commands::Serve { port, host, .. } => {
                assert_eq!(port, 9000);
                assert_eq!(host, "0.0.0.0");
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn serve_requires_port() {
        let error = Cli::try_parse_from(["acp-agent", "serve", "demo-agent"]).unwrap_err();

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn exit_from_status_returns_success_for_zero_exit() {
        assert_eq!(exit_from_status(success_exit_status()), CliExit::Success);
    }

    #[test]
    fn exit_from_status_returns_process_code_for_non_zero_exit() {
        assert_eq!(exit_from_status(exit_status_with_code(5)), CliExit::Code(5));
    }

    #[cfg(unix)]
    #[test]
    fn exit_from_status_returns_signal_convention_for_signal_exit() {
        assert_eq!(exit_from_status(signal_exit_status(15)), CliExit::Code(143));
    }

    fn success_exit_status() -> ExitStatus {
        exit_status_with_code(0)
    }

    #[cfg(unix)]
    fn exit_status_with_code(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;

        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status_with_code(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;

        ExitStatus::from_raw(code as u32)
    }

    #[cfg(unix)]
    fn signal_exit_status(signal: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;

        ExitStatus::from_raw(signal)
    }
}
