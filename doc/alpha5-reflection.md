# Alpha 5 Reflection Notes

This is a critique-first preparation note for the next alpha. It focuses on
what is weak or risky in the current implementation, not on expanding the
feature list.

## Current Judgment

GenieClaw is moving in the right direction for the Genie ecosystem: it is
local-first, Jetson-conscious, memory-aware, and increasingly policy-driven.
The strongest recent work is the runtime contract, actuation policy, skill
manifest audit, tool audit, and support bundle path.

The main risk is not missing features. The main risk is allowing the agent
layer to become a large, trusted monolith before lower runtime boundaries are
ready.

## Architecture Critique

1. The HTTP API was too exposed for a physical agent.
   - A local assistant that can touch memory, tools, and home actuation should
     not bind to the LAN by default.
   - Next-alpha change: default `[core].bind_host` is now `127.0.0.1`; operators
     must explicitly opt into `0.0.0.0`.

2. `server.rs`, `tools/dispatch.rs`, and `memory/mod.rs` are too large.
   - They are still understandable, but they combine routing, policy, execution,
     persistence, and response formatting.
   - Alpha 5 should split by stable seams only: request origin/auth, tool audit,
     memory management API, and home runtime handoff.

3. Request handling is still sequential.
   - This keeps SQLite ownership simple, but a long LLM turn can delay health,
     dashboard, confirmation, and memory API responses.
   - Do not blindly spawn per connection until shared state is wrapped behind
     safe concurrency boundaries.
   - Preferred next step: isolate read-only health/runtime endpoints first, then
     decide whether conversation writes need a serialized actor.

4. Transitional Home Assistant support is still useful but strategically
   dangerous.
   - The code mostly keeps HA behind provider boundaries, but product language
     and tests must keep reinforcing that HA is not the final home runtime.
   - Alpha 5 should define the `genie-home-runtime` client trait shape before
     adding more HA-specific behavior.

5. Voice speaker identity was scaffolded but cleanup order was wrong.
   - The captured WAV was deleted before the future biometric recognizer could
     inspect it.
   - Next-alpha change: identity now receives the WAV path before cleanup.

6. Skill policy is still audit-heavy, not sandbox-heavy.
   - Manifest and signature-material presence are useful, but they are not
     security isolation.
   - Alpha 5 should keep native skills disabled or tightly allowlisted in
     untrusted deployments until process isolation exists.

## Alpha 5 Priorities

The next alpha should be a reliability and boundary release.

1. Secure local surface by default.
   - Localhost bind by default.
   - Clear warning when binding wildcard.
   - Origin headers from first-party clients.
   - Document reverse-proxy or gateway expectations.

2. Split control-plane code at real seams.
   - Move runtime contract and health response composition out of `server.rs`.
   - Move tool audit and policy helpers out of `tools/dispatch.rs`.
   - Keep behavior unchanged while reducing file size and review risk.

3. Make voice pipeline measurable.
   - Persist per-turn timing: record, STT, quick route, LLM, TTS, total.
   - Surface recent voice failures in support bundles.
   - Keep `/no_think` default for voice unless explicit deep reasoning is needed.

4. Prepare home-runtime boundary.
   - Define a narrow trait/API for device graph, status, proposed actuation, and
     final actuation result.
   - Keep Home Assistant as one adapter behind that boundary.

5. Finish operational hardening.
   - Add an operator command that prints runtime contract and policy status in a
     human-readable form.
   - Add release checks for bind host, runtime drift, tool policy, skill policy,
     and actuation audit availability.

## Non-Goals For Alpha 5

- Do not add broad marketplace behavior.
- Do not add cloud account features.
- Do not implement emotion detection.
- Do not expand Home Assistant-specific product assumptions.
- Do not turn `genie-core` into `genie-home-runtime` or `genie-ai-runtime`.

## Exit Criteria

Alpha 5 is ready when:

- default deployment does not expose `genie-core` beyond localhost
- support bundle captures enough runtime state to diagnose a failed field box
- voice identity and multilingual routing have no obvious lifecycle bugs
- unsafe physical action paths are blocked without relying on prompt obedience
- the next extraction boundary for `genie-home-runtime` is documented and tested
