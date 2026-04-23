# Overview

## Purpose

GenieClaw is the local software brain for GeniePod Home.

The repo is optimized around a narrow goal:

- run locally on Jetson-class hardware
- keep the system understandable and debuggable
- provide everyday household usefulness before broad platform ambition
- preserve privacy, bounded behavior, and graceful degradation

This is not a cloud orchestration shell and not a generic agent runtime.

## Main Runtime Modes

`genie-core` supports three primary modes:

1. HTTP server mode
   The default daemon mode. It serves the local chat UI, local API consumers,
   OpenAI-compatible adapters, and direct tool surfaces.
2. REPL mode
   When stdin is interactive, `genie-core` starts a local text REPL instead of
   daemon-only behavior.
3. Voice mode
   Enabled by config, `--voice`, or `GENIEPOD_VOICE=1`. The voice loop runs a
   microphone -> STT -> prompt/tool execution -> TTS pipeline.

In daemon mode, Telegram can also be enabled as a side-channel adapter.

## High-Level Process Topology

Typical Jetson deployment:

```text
llama-server (:8080)
        ^
        |
genie-core (:3000) <---- genie-ctl
        |
        +---- local chat UI / OpenAI-compatible clients
        +---- optional Telegram adapter
        +---- optional Home Assistant provider
        +---- optional ESP32-C6 connectivity controller boundary

genie-governor ---- controls service modes and pressure response
genie-health   ---- polls health endpoints and stores health history
genie-api      ---- serves dashboard/status data
```

## Core User Flows

### Chat HTTP Flow

1. Client sends `POST /api/chat` or `POST /v1/chat/completions`.
2. `genie-core` appends the user message to the conversation store.
3. Fast-path routing may intercept deterministic requests.
   Examples: time, memory diagnostics, system status, explicit web search.
4. If not intercepted, GenieClaw builds the system prompt and injects relevant memory.
5. The LLM returns plain text or tool JSON.
6. If tool JSON is detected, the tool dispatcher executes it.
7. The result is either returned raw or summarized, depending on tool type.
8. Memory auto-capture runs on the user message.

### Voice Flow

1. Record audio from ALSA or auto-detected device.
2. Apply DSP, gating, and optional cleanup.
3. Send audio to Whisper CLI/server.
4. Detect language if configured as `auto`.
5. Run the same routing/prompt/tool pipeline as text.
6. Use Piper for spoken output, optionally with language-specific voices.

### Governor Flow

1. Poll tegrastats and memory state.
2. Track day/night/media modes.
3. Stop or defer optional services under pressure.
4. Expose status and mode change control through the Unix control socket.

## Data At Rest

The runtime primarily stores data under `data_dir`.

Main current databases:

- `memory.db`: persistent memory and FTS-backed recall
- `conversations.db`: conversation history and exports
- `governor.db`: tegrastats history and governor state data
- `health.db`: service health history

The default production `data_dir` is `/opt/geniepod/data`.
The default development `data_dir` is `./data`.

## Why The Repo Is Split Into Small Crates

The crate split is pragmatic, not academic.

- `genie-common` keeps config and shared types reusable across binaries.
- `genie-core` contains the actual AI runtime and most product logic.
- `genie-governor`, `genie-health`, and `genie-api` stay small and operationally separate.
- `genie-ctl` gives you a narrow operator interface without needing a browser.
- `genie-skill-sdk` keeps the native skill ABI explicit.

The result is easier bring-up on Jetson and clearer failure boundaries.
