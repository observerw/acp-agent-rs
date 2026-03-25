use std::process::ExitStatus;

use anyhow::Result;

use crate::runtime::stdio;

pub async fn run_agent(agent_id: &str, user_args: &[String]) -> Result<ExitStatus> {
    stdio::run_agent_stdio(agent_id, user_args).await
}
