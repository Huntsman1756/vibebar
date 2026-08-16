# VibeBar threat model

## Assets

- ChatGPT subscription state and reset windows.
- Provider/model usage aggregates.
- Project/task identifiers and orchestration outcomes.
- Local Codex and OpenCode sessions used by their respective CLIs.

VibeBar must not collect prompts, responses, source code, diffs, browser cookies, API keys, access tokens, or copied authentication files.

## Trust boundaries

### Tauri frontend → Rust commands

Only the bundled `main` window receives the default capability. Event ingestion uses a strict typed structure, rejects unknown fields, caps each batch at 1,000 events, and writes only to the app-owned telemetry path. The CSP allows bundled resources and Tauri IPC only.

### Rust host → Codex App Server

VibeBar launches the installed `codex app-server --stdio` with fixed arguments and requests only `account/rateLimits/read`. It does not inspect `auth.json`. The subprocess environment removes variables whose names look like keys, tokens, secrets, passwords, or credentials. Output is untrusted JSON, capped at 8 MiB, and time-bounded to ten seconds.

This remains a personal-subscription trust boundary: the installed Codex binary can use the user's existing host login. Compromise of that binary is outside VibeBar's process-level isolation and must be handled through binary provenance and normal host controls.

### Rust host → OpenCode

VibeBar launches `opencode stats --pure --days 30 --models` with fixed arguments. `--pure` disables external plugins for the collection run. The command receives the same credential-scrubbed environment, has a 25-second timeout, and has an 8 MiB combined-output ceiling. Its terminal output is parsed as untrusted, version-sensitive input; parse failure is visible and never triggers an alternate credential source.

### Local telemetry producer → JSONL store

V1 assumes producers run as the same OS user. File tampering by that user can alter dashboard metrics. The reader bounds file and line sizes, validates every event, ignores duplicate IDs, and reports malformed rows. V1 is intentionally not suitable for cross-user, remote, or compliance-grade evidence.

## Failure policy

- Every collector is independent; one failure cannot suppress other providers.
- Missing data is `error` or unavailable, never zero and never an implicit fallback.
- VibeBar never writes provider configuration or authentication state.
- There is no HTTP listener, cloud relay, browser-cookie importer, or remote analytics endpoint.
- Opening links and arbitrary shell commands are not capabilities of the frontend.

## Future review gates

Require a new security review before adding browser-cookie extraction, API-key storage, remote ingestion, auto-update, cross-machine synchronization, or a hosted relay. Signed telemetry should be a new schema version with replay and producer-binding tests.
