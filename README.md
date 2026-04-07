# acp-agent

CLI for discovering, installing, launching, and proxying ACP agents from the public registry.

## Install

```sh
curl -fsSL https://github.com/observerw/acp-agent-rs/releases/latest/download/install.sh | sh
```

## Usage

`acp-agent install <agent-id>` prepares binary agents inside the local `acp-agent`
cache so later `run` and `serve` invocations can reuse the downloaded payload.

`acp-agent run <agent-id>` and `acp-agent serve <agent-id>` automatically use
that cache and will populate it on demand when it is missing or stale.
