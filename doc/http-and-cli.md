# HTTP And CLI Reference

## `genie-core` HTTP API

Served by `crates/genie-core/src/server.rs`.

Default bind:

- `0.0.0.0:3000`

### UI And Chat Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Local chat UI |
| `POST` | `/api/chat` | Normal chat turn |
| `POST` | `/api/chat/stream` | Streaming chat turn using NDJSON events |
| `GET` | `/api/chat/history` | Messages for the current conversation |
| `POST` | `/api/chat/clear` | Start a new conversation |
| `GET` | `/api/conversations` | List all stored conversations |
| `GET` | `/api/chat/export?id=<id>` | Export one conversation as JSON |

### Tool And Status Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/tools` | List built-in and loaded tool definitions |
| `GET` | `/api/web-search` | Web search config/cache status |
| `POST` | `/api/web-search` | Execute direct web search |
| `GET` | `/api/health` | Rich runtime health |
| `GET` | `/api/connectivity` | Connectivity controller health and capabilities |

### Compatibility Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/v1/chat/completions` | OpenAI-compatible local bridge |
| `GET` | `/v1/models` | Minimal model listing |

## Core Response Shapes

### `POST /api/chat`

Request:

```json
{"message":"what time is it?"}
```

Response:

```json
{
  "response": "...",
  "tool": "get_time",
  "conversation_id": "..."
}
```

`tool` is omitted or `null` when no tool was used.

### `POST /api/chat/stream`

Streaming events are newline-delimited JSON.

Current event types:

- `token`: normal streamed text
- `replace`: replace prior text with final tool-backed response
- `done`: final event with `response`, optional `tool`, and `conversation_id`

### `GET /api/health`

Current top-level fields:

- `status`
- `llm`
- `memories`
- `conversations`
- `mem_available_mb`
- `connectivity`
- `web_search`
- `version`

### `POST /api/web-search`

Request:

```json
{"query":"ESP32-C6 Thread support","limit":3,"fresh":false}
```

Current response shape:

```json
{
  "tool": "web_search",
  "success": true,
  "query": "ESP32-C6 Thread support",
  "provider": "duckduckgo",
  "fresh": false,
  "cached": false,
  "blocked": false,
  "result_count": 3,
  "items": [
    {
      "title": "Example",
      "text": "Example result text",
      "url": "https://example.test"
    }
  ],
  "response": "Web search results for ..."
}
```

If a query is blocked as sensitive, the endpoint still returns `200` with
`blocked: true` and `result_count: 0`.

### `POST /v1/chat/completions`

Supported request shape:

- OpenAI-style `messages`
- optional `model`
- optional `max_tokens`

Current implementation returns one assistant message in the `choices` array.
Token accounting fields are present but currently zero-filled.

## `genie-api` Dashboard API

Served by `crates/genie-api/src/routes.rs`.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Dashboard HTML |
| `GET` | `/dashboard.js` | Dashboard JavaScript |
| `GET` | `/api/status` | Governor mode, memory, uptime-oriented status |
| `GET` | `/api/tegrastats` | Recent tegrastats history from `governor.db` |
| `GET` | `/api/services` | Latest health state per service from `health.db` |
| `POST` | `/api/mode` | Forward mode change command to governor |

## `genie-ctl` CLI

Implemented in `crates/genie-ctl/src/main.rs`.

### Main Commands

| Command | Purpose |
| --- | --- |
| `genie-ctl status` | System status summary |
| `genie-ctl mode <MODE>` | Change governor mode |
| `genie-ctl chat <MESSAGE>` | Send one chat request |
| `genie-ctl search [--fresh] [--limit N] <QUERY>` | Direct web search |
| `genie-ctl history` | Show current conversation history |
| `genie-ctl tools` | List available tools |
| `genie-ctl connectivity` | Show coprocessor boundary status |
| `genie-ctl skill ...` | Manage loadable skills |
| `genie-ctl health` | Service health report |
| `genie-ctl conversations` | List stored conversations |
| `genie-ctl update-check` | OTA check |
| `genie-ctl diag` | Diagnostics summary |
| `genie-ctl version` | Version output |

### Skill Subcommands

| Command | Purpose |
| --- | --- |
| `genie-ctl skill list` | List installed runtime skills |
| `genie-ctl skill install <SOURCE.so> [DEST_NAME]` | Validate and install a skill |
| `genie-ctl skill remove <SKILL_NAME|FILE_NAME>` | Remove a skill |
| `genie-ctl skill dir` | Print runtime skill directory |

## Current Built-In Tool Families

The exact tool list depends on config and loaded skills, but the built-in
surface currently includes:

- home control and home status
- time
- weather
- web search
- system info
- calculator
- media playback trigger
- memory recall, status, forget, and store
- timers

Memory tools are policy-aware:

- memory recall defaults to shared-room-safe disclosure
- person/private/restricted memories may be withheld unless stronger read context is supplied
- memory status reports canonical artifact counts plus policy-scope counts

## Recommended Reading

- [configuration.md](configuration.md)
- [core-subsystems.md](core-subsystems.md)
- [deployment-and-ops.md](deployment-and-ops.md)
