# Security Policy

LoopDeck is a local-first desktop application that **spawns and supervises an AI
coding agent** (the `claude` CLI) with the ability to read, write, and run
commands on your machine. That makes the agent/subprocess boundary the central
security concern of the product. This document describes the threat model, what
LoopDeck does and does *not* protect against today, and how to report a
vulnerability.

> **This is a 0.1.0 private alpha.** It is unsigned / un-notarized and intended
> for the operator's own machine. Treat any imported project the way you would
> treat running its code.

---

## Reporting a Vulnerability

**Do not open a public GitHub issue or pull request for a security bug.**

Please report vulnerabilities through GitHub's private vulnerability reporting:

👉 **<https://github.com/suprie/loopdeck/security/advisories/new>**

This opens a private, encrypted channel between you and the maintainers. A
GitHub Security Advisory can later be published (with credit) once a fix ships,
and is eligible for a CVE.

**What to include:**

- Affected version (see *Supported Versions* below).
- A clear description of the impact and a minimal reproduction.
- The component you believe is affected (e.g. the subprocess/permission layer,
  a specific IPC command, secret handling).
- Logs are optional and helpful — they live at
  `~/Library/Logs/LoopDeck/loopdeck.log.YYYY-MM-DD` on macOS. **Redact your
  auth token** before sharing; it is never written to the logs by LoopDeck, but
  please confirm.

**Response expectations.** This is a solo, alpha-stage project. We aim to
acknowledge reports within **5 business days** and to coordinate a fix and
disclosure timeline with you. We will not take legal action against good-faith
reporters.

**In scope.** Vulnerabilities in LoopDeck itself — the Rust backend
(`src-tauri/src/`), the Tauri/React frontend, the IPC boundary, the subprocess
and permission model, or secret/state handling.

**Out of scope (report elsewhere):**

- The `claude` CLI itself or the Anthropic API — report to Anthropic.
- Behavior that is the *inherent nature* of running an autonomous agent with
  your OS privileges (e.g. "an agent deleted a file after I approved the
  command"). LoopDeck mitigates *actions* via confirmation; it cannot sandbox
  the model's reasoning. See *Prompt injection* below.
- Vulnerabilities in dependencies — report upstream, then let us know so we can
  bump.

---

## Supported Versions

LoopDesk ships as a single desktop binary. Only the latest release line is
supported with security fixes.

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | ✅ (current alpha)  |
| < 0.1.0 | ❌ (pre-release; upgrade) |

Alpha builds are **not** guaranteed to be mutually compatible — there is no
auto-updater and on-disk shapes may change between alphas. Keep a build you rely
on for rollback. See [`docs/alpha-distribution.md`](docs/alpha-distribution.md).

---

## Trust Model & Boundaries

| Entity | Trust level | Notes |
|--------|-------------|-------|
| **The operator (you)** | Fully trusted | Owns the machine; approves agent actions. |
| **Imported projects** | **Untrusted** | A project directory may contain hostile code, scripts, symlinks, or instructions for the agent. Treat it like untrusted code. |
| **The `claude` CLI** | Trusted dependency, privileged | Spawns with your OS privileges. LoopDeck constrains its *tool use*, not its reasoning. |
| **The model provider / gateway** | Semi-trusted | Reached only through the CLI using a key you provide. LoopDeck never calls the gateway directly. |
| **Other local processes** | Untrusted | Mitigations assume another same-user process may read your files. |

The single most important boundary: **the agent runs with the operator's full OS
privileges.** LoopDeck is a *confirm-first* supervisor, not a sandbox. Every
mitigation below reduces the chance of an undesired action; none of them make it
impossible for an agent you have empowered to damage your machine.

---

## Threat Model

### T1 — Subprocess / PATH hijack from an imported project
A malicious project could ship a `claude` or `git` executable/script and rely on
`$PATH` resolution (including a `.` or empty entry) to make LoopDeck execute it
under its own privileges.

- **Mitigation.** The `binary` module (`src-tauri/src/binary.rs`) resolves
  `claude` and `git` to **absolute, vetted paths** *before* spawning: it skips
  every non-absolute `$PATH` component (closing the cwd/`.`-resolution vector),
  verifies the candidate is a regular executable file, and **pins the result in a
  `OnceLock`** for the process lifetime so a later `$PATH` mutation cannot
  redirect a subsequent spawn. `git` is invoked with `git -C <repo>` (an absolute,
  resolved binary) rather than `Command::new("git").current_dir(repo)`.
- **Residual.** (a) A `$PATH` whose earlier, legitimate entry is itself hostile —
  LoopDeck trusts the *first* vetted absolute match by design. (b) The
  **GUI-launch minimal-`PATH` blind spot**: when launched from Finder/Spotlight,
  Homebrew (`/opt/homebrew/bin`) and npm-global dirs are absent from `$PATH`, so
  `claude` may not be found at all. Launch from a terminal as a workaround. See
  [`docs/alpha-distribution.md`](docs/alpha-distribution.md).

### T2 — Destructive or undesired agent action
An agent that is buggy, or that has been **prompt-injected** via untrusted
content (project files, fetched web pages, issue text), may attempt destructive
commands (`rm -rf`, force-push, overwriting config) or unwanted file edits.

- **Mitigation.** LoopDeck spawns the agent with
  `--permission-mode default` (`src-tauri/src/claude_session.rs`) so that *every*
  tool call not matched by an explicit allow rule emits a `control_request` that
  LoopDeck decides. The decision flow is the four-arm `answer_control_request`:
  1. `AskUserQuestion` prompts are surfaced for the human;
  2. a **destructive floor** (`check_destructive_floor`) denies a curated list of
     destructive command prefixes without further prompt;
  3. `MANUAL_APPROVAL_TOOLS` (Edit/Write/Bash/…) intercept for explicit human
     approval;
  4. the default policy is **`ConfirmChanges`** — confirm first. The generated
     `.claude/settings.json` carries only read-only allow rules; broad
     `Edit(*)`/`Write(*)` and build-runner rules were intentionally removed.
- **Residual.** The destructive floor is an **argv-prefix best-effort deny-list**:
  `mv`/`cp` and composed/chained commands targeting `/`, `/etc`, `/usr`, or
  `$HOME` root are best-effort, and obfuscated destructive commands may not
  match. Because the agent has your full privileges, an agent that gains a broad
  allow rule or chains commands can still cause harm. **Confirmation is the
  mitigation, not a sandbox.** An `AutonomousProject` (no-confirmation) tier is a
  deliberate non-goal for the alpha.

### T3 — Path traversal / symlink escape at the IPC boundary
Project-scoped IPC commands could be fed paths that escape the registered
project root (`../`) or follow a planted symlink to read/write arbitrary files.

- **Mitigation.** Every project-scoped command funnels through the `paths`
  module (`src-tauri/src/paths.rs`): `canonical_root` requires an existing
  directory; `resolve_registered_root` checks the path is a *registered* project;
  `resolve_within` lexically rejects `..` and absolute components *before* any
  filesystem access, then canonicalizes the target (read) or its longest existing
  ancestor (write) and asserts `starts_with(root)` — catching symlink escape in
  both directions. Discovery and `@`-mention walks skip symlinks entirely;
  `walkdir::follow_links(false)` is used for scans.
- **Residual.** Low-value TOCTOU races between canonicalize and use are accepted
  for a local app.

### T4 — Unbounded-input DoS / memory exhaustion
A huge scan tree, a pathological transcript line, or runaway response
accumulation could freeze the UI or exhaust memory.

- **Mitigation.** The `limits` module (`src-tauri/src/limits.rs`) bounds every
  untrusted-input path: scan/search **depth + entry-count + wall-time + result**
  caps that stop-and-return-partial with a warning; `read_bounded_to_string`
  byte caps for README/spec/SKILL/transcript reads; `STREAM_LINE_MAX_BYTES`
  (4 MiB) applied via a cancel-safe bounded line reader on the streaming hot
  path; `ACCUMULATOR_MAX_BLOCKS`/`ACCUMULATOR_MAX_BYTES` that truncate-and-mark
  rather than abort (aborting would orphan the live agent process);
  `PARKED_SLOT_TIMEOUT` (10 min) that reclaims a parked approval/question and
  auto-denies so the agent unblocks.

### T5 — Agent auth token exposure
The model-provider key must not leak into backups, logs, the renderer process,
or other local processes.

- **Mitigation.** The token lives in a dedicated, **owner-only `0600`** file at
  `<config_dir>/agent_token` (macOS:
  `~/Library/Application Support/com.loopdeck.LoopDeck/agent_token`), written
  crash-safe via `persist::atomic_write` and kept **separate from `config.yaml`**
  so it is not mixed into the registry that may be inspected/backed up/shared. It
  is read by `resolve_agent_config` into a local `AgentConfig` only at spawn time
  and set as the child's `ANTHROPIC_AUTH_TOKEN` environment variable — it is never
  held on the long-lived `Mutex<GlobalConfig>`. `get_agent_config` returns only a
  `has_auth_token` presence flag to the renderer, never the plaintext. Diagnostic
  logging never writes the token.
- **Residual.** The token **is** present in the spawned agent's environment by
  design — that is how the agent authenticates. A malicious or prompt-injected
  agent running inside a project can read its own environment and exfiltrate the
  token. This is inherent to delegating model access to a privileged agent;
  mitigate by trusting the operator and by **scoping the token** (use a key with
  the minimum necessary scope/provider-side spend limit). The earlier macOS
  Keychain storage was reverted to the `0600` file because unsigned builds
  re-prompted for the keychain password on every spawn.

### T6 — Corrupted or crash-inconsistent state
A crash mid-write could truncate the registry, and an interrupted turn could be
silently dropped or left looking "busy".

- **Mitigation.** All critical state is written via `persist::atomic_write`
  (temp-file → fsync → same-directory rename). The registry additionally gets a
  `.bak` sibling before every overwrite, and `load_from_path` runs a 4-step
  recovery (primary ok → load; primary malformed → try backup and warn; both
  malformed → hard `exit(1)`, never a silent overwrite with an empty default).
  Interrupted turns reconcile from the transcript: a trailing orphaned `user`
  turn (no reply) is detected and a persisted `interrupted` marker is appended so
  a new turn is unblocked and the UI renders the interruption truthfully.

### T7 — Unsigned / un-notarized build (provenance)
macOS cannot cryptographically verify the binary's provenance, so a tampered
download is indistinguishable from a genuine one.

- **Mitigation (documented, not technical).** Notarization is deferred for
  recurring cost (the Apple Developer Program fee); see the 2026-07-20 decision
  in [`decisions.md`](.loopdeck/decisions.md). The install path therefore
  requires an **explicit Gatekeeper bypass** (`xattr -dr com.apple.quarantine …`
  or right-click → Open), so the operator consciously accepts provenance risk.
  Build from source or download only from the official
  [`suprie/loopdeck`](https://github.com/suprie/loopdeck) repository.

---

## Prompt injection is inherent

LoopDeck reads untrusted content *through* the agent — project files, pages the
agent fetches, issue-tracker text, build output. A capable model can be
instructed by that content to take actions against the operator's interest.
LoopDeck **cannot sandbox the model's reasoning**; it mitigates the model's
*actions* (confirm-first permission model + destructive floor). Operators must:

- **Treat imported projects like running code.**
- Review approval prompts rather than reflexively approving; prefer the narrow
  "Always allow" rules over broad ones, and only for projects you trust.
- Scope the auth token on the provider side (spend limits, restricted keys).

---

## Where your data lives (macOS)

| Data | Path | Notes |
|------|------|-------|
| Registry + config | `~/Library/Application Support/com.loopdeck.LoopDeck/` | `config.yaml` + `.bak` |
| Agent auth token | `…/com.loopdeck.LoopDeck/agent_token` | `0600`, owner-only |
| Per-project memory | `<repo>/.loopdeck/` | Travels with the repo |
| Transcripts | `<repo>/.loopdeck/` | NDJSON conversation history |
| Logs | `~/Library/Logs/LoopDeck/loopdeck.log.YYYY-MM-DD` | Override via `LOOPDECK_LOG_DIR` |

On Linux / headless the config dir falls back to `~/.config/loopdeck/`.

---

## Hardening roadmap

This policy reflects the **hardened alpha** posture. The authoritative list of
shipped hardening and remaining work lives in:

- [`docs/PRD-trust-boundary-hardening.md`](docs/PRD-trust-boundary-hardening.md)
- [`.loopdeck/decisions.md`](.loopdeck/decisions.md) — architectural decisions
  behind each mitigation above
- [`.loopdeck/loops.md`](.loopdeck/loops.md) — release-gate status

Notable items explicitly **deferred** (tracked, not yet shipped): signed /
notarized artifacts (T7), a macOS App Sandbox, a stricter destructive-command
analyzer, and bounded log retention.
