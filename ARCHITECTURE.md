# genie-core — Detailed Architecture

**Version:** 1.0.0-alpha.3 | **Language:** Rust 2024 | **License:** AGPL-3.0-only
**Lines:** ~11,500 | **Tests:** 186 | **Binaries:** 5 (9.8 MB total)

---

## System diagram

```
  Browser (:3000)     genie-ctl      future channel adapters
       │                   │                    │
       ▼                   ▼                    ▼
  ┌─────────────────────────────────────────────────────┐
  │              genie-core (2.4 MB)                  │
  │                                                      │
  │  ┌─── HTTP Server (:3000) ─────────────────────────┐│
  │  │ POST /api/chat         → LLM + tool dispatch    ││
  │  │ GET  /api/chat/history → conversation store      ││
  │  │ POST /api/chat/clear   → new conversation        ││
  │  │ GET  /api/conversations → list all sessions      ││
  │  │ GET  /api/chat/export  → JSON export             ││
  │  │ GET  /api/tools        → list 11 tools            ││
  │  │ GET  /api/health       → system status            ││
  │  │ GET  /                 → chat web UI              ││
  │  └─────────────────────────────────────────────────┘│
  │                                                      │
  │  ┌─── Modules ─────────────────────────────────────┐│
  │  │                                                  ││
  │  │  llm/client.rs ──→ llama.cpp :8080 (OpenAI API) ││
  │  │  llm/retry.rs  ──→ retry + graceful degradation  ││
  │  │                                                  ││
  │  │  tools/dispatch.rs ──→ 8 compiled tools          ││
  │  │  tools/parser.rs   ──→ extract JSON from LLM     ││
  │  │  tools/calc.rs     ──→ math evaluator            ││
  │  │  tools/weather.rs  ──→ Open-Meteo API            ││
  │  │  tools/home.rs     ──→ HA REST + fuzzy match     ││
  │  │  tools/timer.rs    ──→ countdown timers          ││
  │  │  tools/system.rs   ──→ /proc + governor socket   ││
  │  │                                                  ││
  │  │  ha/client.rs ──→ Home Assistant :8123 (REST)    ││
  │  │                                                  ││
  │  │  memory/mod.rs ──→ SQLite + FTS5 (persistent)    ││
  │  │  conversation.rs ──→ SQLite (multi-session)      ││
  │  │  context.rs ──→ window mgmt + summarization      ││
  │  │  prompt.rs ──→ model-aware system prompts        ││
  │  │  ota/mod.rs ──→ GitHub Releases check            ││
  │  │                                                  ││
  │  │  voice/stt.rs ──→ Whisper subprocess (future)    ││
  │  │  voice/tts.rs ──→ Piper subprocess (future)      ││
  │  │  voice/pipeline.rs ──→ full voice flow (future)  ││
  │  │  voice/format.rs ──→ strip markdown for TTS      ││
  │  │                                                  ││
  │  │  repl.rs ──→ interactive terminal mode            ││
  │  │  server.rs ──→ HTTP server (daemon mode)         ││
  │  └─────────────────────────────────────────────────┘│
  └──────────────────────┬──────────────────────────────┘
                         │ Unix socket
  ┌──────────────────────▼──────────────────────────────┐
  │           genie-governor (2.4 MB)                 │
  │                                                      │
  │  governor.rs ──→ mode state machine                  │
  │  control.rs  ──→ Unix socket (/run/geniepod/gov.sock)│
  │  service_ctl.rs ──→ systemctl start/stop             │
  │  store.rs    ──→ SQLite (tegrastats + transitions)   │
  │  tegra_reader.rs ──→ spawn tegrastats, parse output  │
  └──────────────────────────────────────────────────────┘

  ┌──────────────────────┐  ┌────────────────────────────┐
  │ genie-health      │  │ genie-api (2.3 MB)      │
  │ (2.2 MB)             │  │                            │
  │ checker.rs ──→ poll  │  │ http.rs ──→ raw TCP server │
  │   /api/health every  │  │ routes.rs ──→ 5 endpoints  │
  │   30s, forward       │  │ dashboard/ ──→ Chart.js    │
  │   optional alerts    │  │   (embedded via include!)  │
  └──────────────────────┘  └────────────────────────────┘

  ┌──────────────────────┐
  │ genie-ctl (531 KB)│
  │                      │
  │ 10 commands:         │
  │  status, mode, chat  │
  │  history, tools      │
  │  health, convos      │
  │  update-check, diag  │
  │  version             │
  └──────────────────────┘
```

---

## Crate-by-crate detail

### genie-common (library)

Shared types used by all other crates.

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| config.rs | 278 | — | TOML config: `Config`, `CoreConfig`, `GovernorConfig`, `HealthConfig`, `ServicesConfig`. Defaults for all fields. Loads from `GENIEPOD_CONFIG` env or `/etc/geniepod/geniepod.toml`. |
| mode.rs | 91 | — | `Mode` enum (Day, NightA, NightB, Media, Pressure). Per-mode: `required_services()`, `stopped_services()`, `llm_model()`. |
| tegrastats.rs | 215 | 6 | Parse `tegrastats` output: RAM, swap, GPU freq, CPU cores, temps, power. `mem_available_mb()` reads `/proc/meminfo`. |

### genie-core (2.4 MB binary)

The voice AI orchestrator. 21 source files.

#### LLM client (`llm/`)

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| client.rs | 275 | 3 | `LlmClient`: OpenAI-compatible API to llama.cpp. `chat()` (blocking) and `chat_stream()` (SSE). Raw TCP HTTP — no reqwest/hyper. |
| retry.rs | 225 | 5 | `RetryLlmClient`: wraps client with retry (configurable attempts + delay), timeout, graceful fallback messages when LLM restarts. |

**Key design:** Raw TCP instead of an HTTP library keeps the binary under 2.5 MB. The streaming parser handles SSE `data:` lines and `[DONE]` terminator.

#### Home Assistant client (`ha/`)

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| client.rs | 233 | 4 | `HaClient`: GET/POST to HA REST API with Bearer token. `find_entity()` uses fuzzy matching (substring + word overlap + prefix score, threshold 0.4). |

**Key design:** Fuzzy matching means "turn on the living room light" works even if the entity is named "Living Room Ceiling Light." No exact name required.

#### Tool system (`tools/`)

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| dispatch.rs | 371 | 4 | `ToolDispatcher`: routes tool calls to handlers. `tool_defs()` returns all 8 tool schemas for the system prompt. `execute()` dispatches by name. |
| parser.rs | 179 | 8 | `try_tool_call()`: extracts JSON from LLM output. Handles raw JSON, markdown code blocks (```json ... ```), embedded in prose, and `{"tool":...}` vs `{"name":...}` variants. |
| calc.rs | 226 | 7 | Recursive descent math parser: +, -, *, /, parentheses, decimals, unary minus. Division by zero detection. |
| weather.rs | 264 | 1 | Open-Meteo free API: geocoding (city → lat/lon) + current weather + 7-day forecast. WMO weather codes → human descriptions. |
| home.rs | 99 | — | `control()`: HA service calls (turn_on, turn_off, toggle, set_brightness, set_temperature, lock, unlock). `status()`: entity state with attribute details. |
| timer.rs | 63 | — | `TimerManager`: in-memory countdown timers with labels. `check_fired()` returns expired timers. |
| system.rs | 68 | — | `system_info()`: memory from `/proc/meminfo`, uptime from `/proc/uptime`, governor mode via control socket, load from `/proc/loadavg`. |

**Key design:** `ToolCall` has `#[serde(alias = "tool")]` on the `name` field so both `{"tool":"get_time"}` and `{"name":"get_time"}` work — different LLMs use different formats.

#### Household memory system

```text
┌─────────────────────────────────────────────┐
│ Permanent Memory (promoted = true)           │
│ Facts that survived dreaming — never decay   │
├─────────────────────────────────────────────┤
│ Recall Tracker                               │
│ recall_count, max_score per memory           │
│ 4-component weighted promotion scoring       │
├─────────────────────────────────────────────┤
│ Short-Term (memories table + FTS5)           │
│ BM25 search + exponential temporal decay     │
│ Half-life: 30 days (configurable)            │
└─────────────────────────────────────────────┘
```

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| memory/mod.rs | ~460 | 10 | `Memory`: SQLite + FTS5 + temporal decay + categories. `store()`, `store_evergreen()`, `search()` (BM25 * decay), `recent()`, `get_by_kind()`, `delete_by_id()`, `delete_matching()`, `has_similar()`, `promotion_candidates()`, `mark_promoted()`, `prune_decayed()`. Categories: identity, preference, relationship, fact, context. |
| memory/extract.rs | ~250 | 15 | Auto-capture: 15+ patterns extract facts from user text (name, age, job, location, preferences, relationships). `extract_facts()` + `extract_and_store()` with deduplication. |
| memory/inject.rs | ~80 | 4 | Per-query context injection: identity always injected, FTS5 search for query-relevant memories, deduplication across categories. |
| memory/decay.rs | 94 | 9 | `exponential_decay()`: exp(-ln2/halfLife * ageDays). `bm25_rank_to_score()`: normalize FTS5 rank to 0-1. Decay curve validation covers expected half-life behavior (7d=0.85, 30d=0.50, 90d=0.13). |
| memory/recall.rs | 193 | 3 | Dreaming consolidation: `dream_cycle()` runs score→promote→prune. 4-component weighted scoring: frequency(0.30) + relevance(0.35) + recency(0.20) + consolidation(0.15). `PromotionCandidate` with per-component breakdown. |
| conversation.rs | 342 | 6 | `ConversationStore`: multi-session persistent conversations. `create()`, `append()`, `get_messages()`, `get_recent()`, `list()`, `delete()`, `export_json()`. Auto-titles from first user message. |
| context.rs | 226 | 5 | `ContextManager`: keeps LLM context within token limits. Old messages summarized by LLM into 2-3 sentences. Summary injected as system message. Token estimation (1 token ≈ 4 chars). |

**Key designs:**
- **Temporal decay:** `score(t) = bm25_score * exp(-ln2/30 * age_days)`. At half-life (30 days), relevance halves. Evergreen and promoted memories exempt.
- **Dreaming cycle:** Called by governor during night mode. Scores candidates by frequency/relevance/recency/consolidation → promotes top scorers to permanent → prunes decayed below threshold.
- **Recall tracking:** Every `search()` call increments `recall_count` and updates `max_score`. Memories recalled 3+ times from diverse queries become promotion candidates.
- All use SQLite with WAL mode. FTS5 built into SQLite — no external search engine.

#### Prompt system

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| prompt.rs | 253 | 7 | `PromptBuilder`: auto-detects model family from filename (Nemotron, Llama, Qwen, Phi, Small, Generic). Two templates: capable models (JSON schema) and simple models (explicit examples). |

**Key design:** TinyLlama needs `EXAMPLES:` with exact JSON for each tool. Nemotron handles `## Tool Calling` with JSON schema. The prompt adapts automatically.

#### Voice pipeline (future — built, not yet wired to audio)

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| stt.rs | 295 | 3 | `SttEngine`: Whisper subprocess manager. Server mode (HTTP POST) + CLI mode (one-shot). PCM-to-WAV helper (44-byte header). |
| tts.rs | 263 | 2 | `TtsEngine`: Piper subprocess manager. Pipe mode (stdin→PCM stdout, low latency) + file mode (WAV). `play_pcm()` via aplay. |
| pipeline.rs | 232 | 1 | `VoicePipeline`: Audio → STT → LLM/Tools → TTS → Speaker. Full flow with tool call extraction. |
| format.rs | 256 | 9 | `for_voice()`: strips markdown (bold, headers, code blocks, links), normalizes whitespace, truncates to 3 sentences, cleans TTS-unfriendly characters. |

#### Security

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| security/audit.rs | ~200 | 4 | Startup audit: filesystem permissions (world-readable/writable), symlink detection, plaintext secret detection in config, root process warning, localhost binding check. Severity: Critical/Warning/Info. |
| security/env_sanitize.rs | ~150 | 6 | Block 60+ sensitive env vars from tool execution. Exact match (OPENAI_API_KEY, HA_TOKEN, LD_PRELOAD...), suffix patterns (_KEY, _SECRET, _TOKEN...), prefix patterns (AWS_, AZURE_, GOOGLE_...). `sanitized_env()` for subprocess spawning. |
| security/sandbox.rs | ~250 | 9 | **Landlock** filesystem sandbox (Linux 5.13+, near-zero userspace cost): restricts process to config_dir (read) + data_dir (write) + bin_dir (exec). **Inference route validation**: rejects non-localhost URLs (SSRF prevention). **Output sanitization**: redacts API keys, JWT tokens, AWS keys, GitHub tokens from LLM responses. |

**Key design:** GeniePod favors kernel-native controls and narrow local interfaces over heavyweight policy and networking layers. The goal is a home appliance security model that is easier to reason about, cheaper to run, and safer by default.

#### OTA

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| ota/mod.rs | 307 | 5 | `OtaManager`: check GitHub Releases for updates. Version comparison (semver, strips pre-release). Backup + rollback for binary replacement. |

#### Server + REPL

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| server.rs | 361 | — | `ChatServer`: HTTP server on `:3000`. 9 endpoints including rich `/api/health`. Sequential request handling. |
| repl.rs | 111 | — | Interactive REPL: streaming LLM output, tool dispatch, persistent conversations. Auto-detected via `isatty(0)`. |
| lib.rs | 60 | — | **Public library API.** Re-exports: `LlmClient`, `Message`, `HaClient`, `Memory`, `ToolDispatcher`, `ToolCall`, `ToolResult`, `ConversationStore`, `PromptBuilder`. Any Rust project can `use genie_core::*`. |
| main.rs | ~90 | — | GeniePod binary. Thin wrapper: loads config, builds components from lib, routes to REPL or HTTP server. |

---

### genie-governor (2.4 MB binary)

Memory governor daemon.

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| governor.rs | 405 | 7 | `Governor`: main loop. Reads `/proc/meminfo` every 5s. Determines target mode (pressure > media > time-based). Transitions: stop/start services, swap LLM model. |
| control.rs | 120 | — | Unix domain socket at `/run/geniepod/governor.sock`. JSON commands: `set_mode`, `media_start`, `media_stop`, `status`. |
| service_ctl.rs | 141 | — | `ServiceCtl`: `systemctl start/stop`, `docker stop/start`, `swap_llm_model()` (writes systemd drop-in override), `enable_zram()`. |
| store.rs | 101 | — | SQLite store: `tegrastats` table (24h retention, hourly prune) + `mode_transitions` table. |
| tegra_reader.rs | 76 | — | Spawns `tegrastats --interval N`, parses each line via `genie_common::tegrastats`, broadcasts via tokio watch channel. |
| main.rs | 37 | — | Entry point. Loads config, opens DB, starts governor loop. |

**Key design:** `sd_notify` is implemented as 2 lines of raw Unix datagram — no libsystemd dependency. LLM model swap writes a systemd drop-in config and restarts the service, so systemd manages the actual process lifecycle.

---

### genie-health (2.2 MB binary)

Service health monitor.

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| checker.rs | 298 | — | `HealthMonitor`: polls HTTP endpoints every 30s. SQLite logging (24h retention). Alert dedup (1st + every 10th failure). Optional local alert webhook forwarding via raw TCP POST. |
| main.rs | 22 | — | Entry point. |

---

### genie-api (2.3 MB binary)

System dashboard.

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| http.rs | 115 | — | Raw TCP HTTP/1.1 server on `:3080`. Routes to handlers. CORS headers. |
| routes.rs | 210 | — | `get_status()` (governor socket), `get_tegrastats()` (SQLite), `get_services()` (health SQLite), `post_mode()` (governor socket), `serve_dashboard()` / `serve_dashboard_js()` (embedded HTML). |
| main.rs | 36 | — | Entry point. |

Dashboard files (compiled into binary):
- `dashboard/index.html` — dark theme, Chart.js, service health table, mode badge
- `dashboard/dashboard.js` — polls every 5s, backfills history, 4 real-time charts

---

### genie-ctl (531 KB binary)

CLI management tool.

| File | Lines | Tests | Purpose |
|------|-------|-------|---------|
| main.rs | 536 | 1 | 10 commands: `status`, `mode`, `chat`, `history`, `tools`, `health`, `conversations`, `update-check`, `diag`, `version`. HTTP client (raw TCP) + governor socket client. |

---

## Data flow: "turn on the living room light"

```
1. User types in browser → POST /api/chat {"message":"turn on the living room light"}
2. server.rs → append to conversation DB → build LLM context (system prompt + history)
3. llm/client.rs → POST to llama.cpp :8080/v1/chat/completions
4. LLM returns: {"tool":"home_control","arguments":{"entity":"living room light","action":"turn_on"}}
5. tools/parser.rs → extract JSON → parse into ToolCall
6. tools/dispatch.rs → route to exec_home_control()
7. tools/home.rs → ha/client.rs → find_entity("living room light") via fuzzy match
8. ha/client.rs → POST to HA :8123/api/services/light/turn_on {"entity_id":"light.living_room"}
9. HA turns on the light
10. tools/home.rs → returns "Done. Turned on Living Room Light (light.living_room)."
11. server.rs → append tool result to conversation → ask LLM for summary
12. LLM returns: "I've turned on the living room light for you."
13. server.rs → append to conversation → return JSON response
14. Browser displays: "I've turned on the living room light for you." [TOOL: home_control]
```

## Data flow: governor mode switch to media

```
1. User: "play Inception"
2. LLM returns: {"tool":"play_media","arguments":{"query":"Inception"}}
3. tools/dispatch.rs → exec_play_media() → governor_command({"cmd":"media_start"})
4. governor control.rs → receives command via Unix socket
5. governor.rs → transition(Day → Media)
   a. ServiceCtl::stop("genie-llm.service")  — frees ~2.8 GB
   b. store.rs → log transition to SQLite
6. governor responds: {"ok":true,"mode":"media"}
7. tools/dispatch.rs → returns "Playing: Inception. Switched to media mode."
8. mpv launches with --hwdec for NVDEC hardware decode → HDMI output
9. Later: user says "stop playing"
10. governor_command({"cmd":"media_stop"}) → transition(Media → Day)
11. ServiceCtl::swap_llm_model() → restart genie-llm.service
12. LLM reloads Nemotron 4B (~3-8 seconds)
```

---

## Configuration

```toml
# /etc/geniepod/geniepod.toml

data_dir = "/opt/geniepod/data"

[core]
port = 3000                     # Chat API + web UI
llm_model_name = "nemotron-4b"  # Prompt optimization
ha_token = ""                   # Or set HA_TOKEN env var
max_history_turns = 20          # Conversation context window

[governor]
poll_interval_ms = 5000         # Memory check interval
night_start_hour = 23
day_start_hour = 6
night_model_swap = false        # true → Nemotron 9B at night

[governor.pressure]
stop_optins_mb = 500            # Stop Nextcloud/Jellyfin
reduce_context_mb = 300         # Cap LLM context
swap_stt_mb = 200               # Downgrade to Whisper tiny
zram_mb = 100                   # Enable 2 GB zram (last resort)

[health]
interval_secs = 30
alert_enabled = true
alert_webhook_url = ""

[services.core]
url = "http://127.0.0.1:3000/api/health"
systemd_unit = "genie-core.service"

[services.llm]
url = "http://127.0.0.1:8080/health"
systemd_unit = "genie-llm.service"

[services.homeassistant]
url = "http://127.0.0.1:8123/api/"
systemd_unit = "homeassistant.service"
```

---

## Dependencies (minimal by design)

| Crate | Version | Why |
|-------|---------|-----|
| tokio | 1.x | Async runtime (single-threaded) |
| serde + serde_json | 1.x | JSON serialization |
| toml | 0.8 | Config file parsing |
| rusqlite | 0.32 | SQLite with FTS5 (bundled, no system dep) |
| tracing + tracing-subscriber | 0.1/0.3 | Structured logging |
| anyhow | 1.x | Error handling |
| libc | 0.2 | localtime_r, isatty |

**No HTTP framework. No ORM. No crypto library. No AI framework.**

---

## Build profiles

```toml
[profile.release]
opt-level = "z"     # Optimize for binary size
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit
strip = true        # Strip debug symbols
panic = "abort"     # No unwinding
```

Result: 2.2-2.4 MB per binary with SQLite bundled.
