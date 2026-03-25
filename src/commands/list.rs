use std::error::Error;
use std::fmt;
use std::io::{self, Write};

use crate::registry::{FetchRegistryError, Registry, fetch_registry};

pub async fn list_agents<W: Write>(writer: &mut W) -> Result<(), ListError> {
    let registry = fetch_registry().await.map_err(ListError::FetchRegistry)?;
    write_agent_list(&registry, writer).map_err(ListError::Write)
}

fn write_agent_list<W: Write>(registry: &Registry, writer: &mut W) -> io::Result<()> {
    let mut agents = registry.list_agents().iter().collect::<Vec<_>>();
    agents.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });

    for agent in agents {
        writeln!(
            writer,
            "{}\t{}\t{}",
            agent.name, agent.id, agent.description
        )?;
    }

    Ok(())
}

#[derive(Debug)]
pub enum ListError {
    FetchRegistry(FetchRegistryError),
    Write(io::Error),
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FetchRegistry(error) => write!(f, "{error}"),
            Self::Write(error) => write!(f, "Failed to write agent list: {error}"),
        }
    }
}

impl Error for ListError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FetchRegistry(error) => Some(error),
            Self::Write(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn writes_name_id_and_description_for_each_agent() {
        let registry = Registry::from_value(json!({
            "version": "1",
            "agents": [
                {
                    "id": "z-agent",
                    "name": "Zulu",
                    "version": "1.0.0",
                    "description": "Last agent",
                    "authors": ["Example"],
                    "license": "MIT",
                    "distribution": { "npx": { "package": "@acme/zulu" } }
                },
                {
                    "id": "a-agent",
                    "name": "Alpha",
                    "version": "1.0.0",
                    "description": "First agent",
                    "authors": ["Example"],
                    "license": "MIT",
                    "distribution": { "npx": { "package": "@acme/alpha" } }
                }
            ]
        }))
        .unwrap();

        let mut output = Vec::new();
        write_agent_list(&registry, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Alpha\ta-agent\tFirst agent\nZulu\tz-agent\tLast agent\n"
        );
    }
}
