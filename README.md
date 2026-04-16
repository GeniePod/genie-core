# genie-core

Local voice AI engine for **GeniePod Home**.

`genie-core` is the on-device runtime behind GeniePod Home: a voice-first, local-first
home AI appliance designed for shared spaces, strong local security boundaries, and
appliance-like behavior.

OpenClaw proved people want AI that feels personal, remembers context, and fits into
everyday life. `genie-core` exists to build that next step on a better trust model:
local by default, useful every day, and designed to stay in the home.

## What It Does

- runs the local LLM orchestration loop for chat and voice
- exposes a small HTTP API and web UI for local control
- integrates with Home Assistant for home control and status
- stores conversation history and household memory locally in SQLite
- ships with companion services for health, system control, and status dashboards
- keeps the runtime small and security-conscious for Jetson-class hardware

## Workspace

| Crate | Purpose |
|-------|---------|
| `genie-core` | Local AI runtime: prompt building, tools, memory, voice loop, HTTP API |
| `genie-common` | Shared config, mode types, and tegrastats parsing |
| `genie-ctl` | Local CLI for chat, status, tools, health, and diagnostics |
| `genie-governor` | Resource governor and service lifecycle controller |
| `genie-health` | Local health polling and alert forwarding |
| `genie-api` | Lightweight system dashboard |
| `genie-skill-sdk` | Rust SDK for native shared-library skills |

## Product Direction

- `GeniePod Home` is the first and primary product target
- living-room shared-space use comes before platform breadth
- Home Assistant makes the system stronger, but should not be required for core value
- messaging channels are planned as native adapters later; they are not part of the core runtime architecture
- the product promise is local home AI, not a cloud-routed agent stack

## Quick Start

```bash
# Build and test
make
make test

# Run locally with the development config
GENIEPOD_CONFIG=deploy/config/geniepod.dev.toml cargo run --bin genie-core

# Run the local dashboard
GENIEPOD_CONFIG=deploy/config/geniepod.dev.toml cargo run --bin genie-api
```

For a full walkthrough, see [GETTING_STARTED.md](GETTING_STARTED.md).

## Design Principles

- **Trust over breadth**: local boundaries and predictable behavior matter more than feature count
- **Appliance over stack**: the runtime should feel stable, inspectable, and deployable
- **Usefulness over demo theater**: timers, routines, home control, and memory matter more than flashy tricks
- **Small dependencies**: raw Tokio TCP, bundled SQLite, no heavyweight web or agent frameworks

## Deployment

The current production target is Jetson Orin Nano 8 GB hardware for `GeniePod Home`.
The repo includes:

- Jetson deployment scripts
- systemd units
- default configs
- wake-word helper scripts
- Docker support for local development

## Current Focus

The runtime is being tightened around the `GeniePod Home` product direction:

- local shared-space voice interaction
- household memory
- Home Assistant integration
- stronger security defaults
- native GeniePod-owned channel adapters later

## License

GNU Affero General Public License v3.0

See [LICENSE](LICENSE).
