# Repository Map

## Top Level

| Path | Purpose |
| --- | --- |
| `README.md` | Product summary and quick start |
| `GETTING_STARTED.md` | Bring-up guide |
| `ARCHITECTURE.md` | Higher-level architecture deep dive |
| `CODEBASE.md` | Narrative code walkthrough |
| `CONNECTIVITY.md` | ESP32-C6 boundary and ownership split |
| `VECTOR_MEMORY.md` | Vector-memory design document |
| `ROADMAP.md` | Product roadmap |
| `doc/` | Current documentation entry point |
| `crates/` | Workspace crates |
| `deploy/` | Configs, scripts, systemd units, Docker assets |
| `skills/` | Native skill examples and guide |

## Workspace Crates

| Path | Purpose |
| --- | --- |
| `crates/genie-common` | shared config, mode, tegrastats |
| `crates/genie-core` | main runtime |
| `crates/genie-api` | dashboard/status service |
| `crates/genie-governor` | mode and pressure manager |
| `crates/genie-health` | health polling |
| `crates/genie-ctl` | local CLI |
| `crates/genie-skill-sdk` | loadable skill ABI |

## `crates/genie-core/src`

### Entrypoints

- `main.rs`
- `lib.rs`
- `server.rs`
- `repl.rs`
- `voice_loop.rs`

### Runtime Context And Conversation

- `context.rs`
- `conversation.rs`

### LLM

- `llm/mod.rs`
- `llm/client.rs`
- `llm/retry.rs`

### Prompt And Reasoning

- `prompt.rs`
- `reasoning.rs`

### Home Assistant

- `ha/mod.rs`
- `ha/client.rs`
- `ha/provider.rs`
- `ha/policy.rs`

### Tools

- `tools/mod.rs`
- `tools/dispatch.rs`
- `tools/parser.rs`
- `tools/quick.rs`
- `tools/system.rs`
- `tools/home.rs`
- `tools/timer.rs`
- `tools/calc.rs`
- `tools/weather.rs`
- `tools/web_search.rs`

### Memory

- `memory/mod.rs`
- `memory/extract.rs`
- `memory/inject.rs`
- `memory/policy.rs`
- `memory/recall.rs`
- `memory/decay.rs`

### Profile

- `profile/mod.rs`
- `profile/ingest.rs`
- `profile/toml_profile.rs`

### Security

- `security/mod.rs`
- `security/audit.rs`
- `security/credentials.rs`
- `security/env_sanitize.rs`
- `security/injection.rs`
- `security/loop_guard.rs`
- `security/sandbox.rs`
- `security/taint.rs`

### Voice

- `voice/mod.rs`
- `voice/aec.rs`
- `voice/dsp.rs`
- `voice/format.rs`
- `voice/language.rs`
- `voice/noise.rs`
- `voice/pipeline.rs`
- `voice/streaming.rs`
- `voice/stt.rs`
- `voice/tts.rs`
- `voice/vad.rs`

### Other Runtime Surfaces

- `connectivity/mod.rs`
- `skills/mod.rs`
- `skills/loader.rs`
- `ota/mod.rs`
- `telegram.rs`

## Tests

Current integration-style tests outside `src/`:

- `crates/genie-core/tests/tool_dispatch_test.rs`
- `crates/genie-core/tests/tools_test.rs`

Most other tests are colocated unit tests inside the module files.

## Deploy Tree

### Config

- `deploy/config/geniepod.toml`
- `deploy/config/geniepod.dev.toml`
- `deploy/config/profile.toml.example`

### Systemd

- `deploy/systemd/*.service`
- `deploy/systemd/*.target`

### Scripts

- `deploy/setup-jetson.sh`
- `deploy/scripts/*`

### Docker

- `Dockerfile`
- `docker-compose.dev.yml`
- `deploy/docker/docker-compose.yml`

## Skills Tree

Current important files:

- `skills/SKILL-DEVELOPER-GUIDE.md`

Runtime-loaded skill binaries are not stored under `skills/`; they are loaded
from the runtime skills directory used by `genie-core`.

## Which File To Open First For Common Tasks

| Task | Start Here |
| --- | --- |
| Chat/API behavior | `crates/genie-core/src/server.rs` |
| Prompt/tool selection | `crates/genie-core/src/prompt.rs` and `tools/dispatch.rs` |
| Memory bugs | `crates/genie-core/src/memory/mod.rs` |
| Voice bugs | `crates/genie-core/src/voice_loop.rs` and `voice/` |
| Home Assistant behavior | `crates/genie-core/src/ha/provider.rs` |
| Search behavior | `crates/genie-core/src/tools/web_search.rs` |
| CLI behavior | `crates/genie-ctl/src/main.rs` |
| Governor behavior | `crates/genie-governor/src/governor.rs` |
| Dashboard behavior | `crates/genie-api/src/routes.rs` |

## Recommended Reading

- [overview.md](overview.md)
- [services-and-crates.md](services-and-crates.md)
- [core-subsystems.md](core-subsystems.md)
- [../CODEBASE.md](../CODEBASE.md)
