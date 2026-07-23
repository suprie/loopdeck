# Loops

## Current

- **Started**: 2026-07-05
- **Goal**: Ship a hardened private alpha on one explicitly supported OS. The
  release gate is intentionally narrow: honest agent permissions, crash-safe
  critical state, bounded project/agent input, deterministic interruption
  recovery, basic CI, and a documented install/upgrade path. Public V0.1 adds a
  small cross-boundary regression suite — signed/notarized artifacts were
  dropped for cost (see the 2026-07-20 decision). Broader product maturity work
  remains tracked but does not block the alpha.
- **Status**: in_progress
- **Last completed**: 2026-07-23 — Closed the Gate B step "Add `LICENSE` and `SECURITY.md` with the agent/subprocess threat model and vulnerability-reporting path." `LICENSE` is MIT ("LoopDeck Contributors", 2026) with the `license` field wired into both `package.json` (`"license": "MIT"`) and `src-tauri/Cargo.toml` (`license = "MIT"`) in the same change — a bare LICENSE with no SPDX field is invisible to npm/cargo/license tooling. `SECURITY.md` is the full agent/subprocess threat model: a supported-versions table (0.1.x only), private vuln-reporting via GitHub Security Advisories (`github.com/suprie/loopdeck/security/advisories/new`, 5-business-day ack, explicit in/out-of-scope + log-redaction), a trust-model table, **seven threats T1–T7** each paired with the real mitigation module + its residual risk (T1 subprocess/PATH hijack → `binary` `OnceLock` abs-path resolution; T2 destructive agent action → `--permission-mode default` + 4-arm `answer_control_request` + destructive floor + `ConfirmChanges`; T3 traversal/symlink escape → `paths` containment; T4 unbounded-input DoS → `limits` budgets; T5 auth-token exposure → `0600` `agent_token`; T6 crash/corrupt state → atomic-write + `.bak`; T7 unsigned-build provenance), the "prompt injection is inherent (LoopDeck mitigates actions, not reasoning)" caveat, and a where-your-data-lives table. Every cited symbol was verified against the code — `claude_session.rs:358` is `--permission-mode default`, **not** the stale `acceptEdits` the P5 wording assumed; the SECURITY.md and P5 line were corrected accordingly. `cargo metadata --manifest-path …/Cargo.toml --no-deps` exit 0; `package.json` parses. Also marked the two P5 mirror items `[x]`. Next unchecked Gate B steps: "Provide user-accessible diagnostics and bounded log retention" and "Persist only navigation identifiers/preferences in Zustand." See decision of same date. Previous milestone (2026-07-23): Closed the first **Gate B (Public V0.1)** step: "Define the release artifact pipeline and smoke-test installation plus upgrade/reinstall behavior." Two deliverables landed. (1) `docs/release-pipeline.md` is the **build/release contract** (companion to the `docs/alpha-distribution.md` install contract): names the one SUPPORTED artifact (macOS arm64 `.dmg`) vs. two EXPERIMENTAL (Linux `.deb`/`.AppImage`, Windows `.exe`/`.msi`), pins version-in-three-places-that-must-agree (`package.json`/`tauri.conf.json`/`Cargo.toml` = `0.1.0`), documents local-vs-CI parity, the five-stage pipeline, the cut-a-release checklist, and the re-enable-signing runbook. `.github/workflows/build.yml` already implements it (`v*` tag → `tauri-action`, macOS arm64 SUPPORTED; signing env vars *omitted* not blanked so the bundler skips signing). (2) **`scripts/smoke-test-release.sh`** is the automated half of the §6 smoke — hermetic mode (default, portable) builds synthetic `.app` skeletons and proves the core "state lives outside the bundle" invariant reduces to a file-system check (install/upgrade/reinstall/rollback never touch the config dir, registry, `agent_token`, or per-repo `.loopdeck/` — byte-for-byte via a path+content `shasum` digest); `--app`/`--dmg` modes additionally assert real bundle structure and (for `--dmg`) `hdiutil` mount/copy/unmount. The rollback step simulates a corrupted registry and proves `cp config.yaml.bak config.yaml` restores the last-known-good. Runs entirely in `mktemp` + `EXIT` trap (never touches real user dirs); fail-stop on first broken invariant. **Verified**: 11/11 assertions in both hermetic and `--app`-against-the-real-`src-tauri/target/release/bundle/macos/LoopDeck.app` runs. Manual GUI sign-off (§6b) remains a separate human step. See decision of same date. Previous milestone: 2026-07-22 — Fixed recurring "Interrupted" bubbles (were silently-timed-out *manual approvals*, not crashes): cross-project permission visibility (`list_pending_permissions`) + truthful `interrupt_kind` (`ApprovalTimeout`/`QuestionTimeout`) on `ConversationTurn`. Next unchecked Gate B steps: "Provide user-accessible diagnostics and bounded log retention" and "Persist only navigation identifiers/preferences in Zustand."

## Next Steps

### Release Gate A — Hardened private alpha

Source: [`docs/PRD-trust-boundary-hardening.md`](../docs/PRD-trust-boundary-hardening.md)

- [x] **Honest permission default:** ship `ConfirmChanges` first; remove generated `Edit(*)`, `Write(*)`, and broad build-runner rules; align Claude spawn settings, LoopDeck policy, approval UI, and regression tests
- [x] **Defer autonomous mode:** do not add per-project `AutonomousProject` configuration until the confirm-first path is proven usable; this is not an alpha blocker
- [x] **Crash-safe critical state:** add one shared atomic-write helper and use it for the registry, `project.yaml`, `loops.md`, PRDs, and generated Claude settings
- [x] **Recoverable registry:** keep one last-known-good backup and never overwrite a malformed primary registry with a fresh default
- [x] **Central project boundary:** resolve every project-scoped IPC request through shared registered-root and contained-relative-path helpers; reject traversal and symlink escape
- [x] **Bound untrusted work:** cap recursive scan depth/entries/time, file and NDJSON line sizes, `ResponseAccumulator` bytes/blocks, and parked approval/question duration
- [x] **Minimal interruption recovery:** after restart or child failure, classify incomplete work as `interrupted`, clear stale busy/waiting state, and allow a new turn; persist a separate run record only if transcript-based recovery proves insufficient
- [x] **Basic CI:** require `cargo fmt --check`, Clippy, `cargo test`, `npm ci`, and `npm run build`; start with the alpha's supported OS rather than a three-OS matrix
- [x] **Clear current lint debt:** resolve existing Clippy failures before enabling `-D warnings`
- [x] **Alpha distribution contract:** name the one supported OS and document installation, upgrade/reinstall, rollback, prerequisites, and diagnostic-log location — `docs/alpha-distribution.md`. macOS on Apple Silicon (arm64) is the one supported OS (unsigned `.dmg`, Gatekeeper bypass on install). Covers prerequisites (`claude` CLI + GUI-launch minimal-PATH caveat, `git` advisory, keychain auth token), install/upgrade/reinstall (manual `.app` replacement — state lives outside the bundle), rollback (restore prior `.app` + `config.yaml.bak` registry recovery), a "where your data lives" table, and the diagnostic log at `~/Library/Logs/LoopDeck/loopdeck.log.YYYY-MM-DD`. Linux/Windows artifacts from `build.yml` are explicitly out of scope (Gate B). Side-fix: corrected `config.rs` doc comments that claimed the registry was at `~/.config/loopdeck/` — on macOS `directories::ProjectDirs::config_dir()` actually resolves to `~/Library/Application Support/com.loopdeck.LoopDeck/` (`~/.config` is the Linux/headless fallback); the contract now states the real path. `cargo fmt --check` clean on config.rs. (The pre-existing untracked `graphify.rs` fmt diff noted when this step ran was resolved in a follow-up `cargo fmt` pass on 2026-07-22 — the Rust CI gates, `cargo fmt --check` and `cargo clippy -- -D warnings`, are now both green.)
- [x] **Alpha smoke test:** manually verify import → start turn → approve/deny → interrupt → restart/recover on a packaged build

### Release Gate B — Public V0.1

- [x] **Define the release artifact pipeline and smoke-test installation plus upgrade/reinstall behavior** (2026-07-23) — Two deliverables: (1) `docs/release-pipeline.md` defines the **build/release contract** — the one supported artifact (macOS arm64 `.dmg`), version-in-three-places-that-must-agree (`package.json` / `tauri.conf.json` / `Cargo.toml`, all `0.1.0` today), the local-vs-CI build paths (same commands → faithful preview), the five-stage pipeline (trigger → frontend → Rust → bundle → publish → smoke), the cut-a-release checklist, and the re-enable-signing runbook (deferred per 2026-07-20). `.github/workflows/build.yml` already implements it (`v*` tag → `tauri-action`, signing env vars omitted so the bundler emits an unsigned `.dmg`, macOS arm64 SUPPORTED + Linux/Windows EXPERIMENTAL). (2) **`scripts/smoke-test-release.sh`** — the automated half of the §6 smoke. Hermetic mode (default, portable, no build) builds synthetic `.app` skeletons and proves the core "state lives outside the bundle" invariant reduces to a file-system check: none of install/upgrade/reinstall/rollback touches the config dir, registry, `agent_token`, or per-repo `.loopdeck/` (byte-for-byte, via a path+content `shasum` digest). Two extra modes — `--app <path>` and `--dmg <path>` — additionally assert the real bundle structure (`Contents/MacOS/LoopDeck` executable + `Contents/Info.plist`) and (for `--dmg`) mount/copy/unmount via `hdiutil`. The rollback step also simulates a corrupted registry and proves the documented `cp config.yaml.bak config.yaml` recovery restores the last-known-good registry. Runs entirely in a `mktemp` dir with an `EXIT` trap — never touches real `~/Library/Application Support/...`, `/Applications`, or any real `.loopdeck/`. Fail-stop: exits non-zero on the first broken invariant. **Verified**: hermetic mode (11 assertions) + `--app` mode against the existing real `src-tauri/target/release/bundle/macos/LoopDeck.app` (11 assertions) both pass. The manual GUI sign-off (§6b) remains a separate human step before announcing a release. See decision of same date.
- [x] Add focused frontend tests for streaming, approval, and interruption state transitions
- [x] Add one automated cross-boundary smoke test covering import → agent approval → interrupt/recovery
- [x] **Add `LICENSE` and `SECURITY.md`** (2026-07-23) — `LICENSE` is MIT ("LoopDeck Contributors", 2026); `license` field added to `package.json` (`"license": "MIT"`) and `src-tauri/Cargo.toml` (`license = "MIT"`) in the same change so the SPDX field is machine-readable. `SECURITY.md` is the agent/subprocess threat model: supported versions (0.1.x only), private vuln reporting via GitHub Security Advisories (5-business-day ack, in/out-of-scope), trust-model table, seven threats (T1 subprocess/PATH hijack → `binary` abs-path `OnceLock`; T2 destructive action → `--permission-mode default` + 4-arm `answer_control_request` + destructive floor + `ConfirmChanges`; T3 traversal/symlink → `paths`; T4 DoS → `limits`; T5 token exposure → `0600` `agent_token`; T6 crash/corrupt state → atomic-write + `.bak`; T7 unsigned provenance), the "prompt injection is inherent" caveat, and a where-data-lives table. Every cited symbol verified in code; `claude_session.rs:358` is `default` (not `acceptEdits`). See ## Current + decision of same date.
- [ ] Provide user-accessible diagnostics and bounded log retention
- [ ] Persist only navigation identifiers/preferences in Zustand; reload project and run state from Rust

### Release Gate C — Auto-commit + Pull Request

Source: [`docs/epics/agent-full-access/prd-verify-and-ship-skills.md`](../docs/epics/agent-full-access/prd-verify-and-ship-skills.md)

Closes the orchestrator's build→verify→ship tail so an agent-led feature
lands as a reviewable PR instead of a pile of staged files. Two focused,
stack-agnostic skills (they operate on any imported project — Go, Android,
PHP, iOS, Ruby, Python, Node, Rust, etc.), plus the orchestrator wiring that
runs them in sequence. The full-access permission tier (Gate D candidate,
same epic) is **not** a prerequisite — auto-commit + PR ship usefully under
`ConfirmChanges` today.

- [ ] **`loopdeck:open-pr` skill** — `.agents/skills/loopdeck-open-pr/SKILL.md`. Pre-flight checks (`gh auth status`, branch ≠ `main`/`master`, upstream pushed), gathers context from `.loopdeck/decisions.md` + `loops.md ## Current` + `git log main..HEAD --oneline`, generates a PR body (Summary / What changed / PRD / Decisions / Test plan), **shows the body to the user for confirmation**, then runs `gh pr create --web`. Test-plan section is inferred from a marker → command table (Go, Rust, Node, Android/JVM, Maven, PHP, Swift, iOS, Ruby, Python, Elixir, unknown fallback) — never hardcoded to one stack. Appends the PR URL to `loops.md ## Next Steps`.
- [ ] **Auto-commit hook point** — Decide where `git add` + `git commit` happen. Lean: the `open-pr` skill owns both (currently the orchestrator only stages with `git add` and never commits; the commit message is authored from the verified scope). The commit groups the feature's WIP commits coherently before push. No auto-push without the PR body confirmation.
- [ ] **`loopdeck:prd-verifier` skill** — `.agents/skills/loopdeck-prd-verifier/SKILL.md`. Read-only (`allowed-tools: [Read, Glob, Grep, Bash]`, no Edit/Write). Parses the PRD's acceptance criteria, identifies changed files via `git diff --name-only main...HEAD` (with per-stack ignore sets), returns a per-criterion PASS/PARTIAL/FAIL table with `file:line` evidence + a non-goals scope-creep audit. Verdict roll-up: any FAIL → BLOCK, any PARTIAL → WARN, all PASS → PASS.
- [ ] **Orchestrator wiring** — In `.agents/skills/loopdeck-orchestrator/SKILL.md`: insert new Phase 6 "Verify Against PRD" (invokes `prd-verifier`, verdict table gates the next phase), renumber existing Phase 6 → Phase 7 "Decide & Open PR" (invokes `open-pr` only on a green verdict). Update the ASCII flow diagram, the Phase 2 plan-template final-phase rows, and the Memory Convention cross-references from "Phase 6" to "Phase 7". `.claude/skills/` mirror is gitignored — edits land in `.agents/skills/` only.
- [ ] **End-to-end smoke** — Run the orchestrator on a small PRD; confirm the sequence produces a verify report → draft PR body → created PR URL. Confirm a BLOCK verdict from Phase 6 prevents the PR step. Confirm `/loopdeck:open-pr` works standalone (without the orchestrator) on a throwaway branch.

### Supporting backlog — not an alpha release gate

The detailed P2/P3/P5/P6 items below remain useful implementation notes. They
only block the private alpha when they directly satisfy a Gate A item above.
Do not expand the release definition by treating every unchecked item as a
prerequisite.

### Supporting work done outside the gates

- [x] **Graphify detect-and-surface** (2026-07-20) — LoopDeck now reads `graphify-out/graph.json` when Graphify has been run in a project, surfacing node/edge/community counts, a confidence breakdown, and god nodes in a new Graph tab. No Python/CLI dependency — LoopDeck only parses JSON output. Scanner badges discovered repos with `has_graphify`. Follow-ups (triggering builds, managing the MCP server, in-app querying) explicitly deferred. See decision of same date.
- [x] **Surface "stuck" AskUserQuestion prompts globally** (2026-07-21) — A LoopDeck-spawned agent that called `AskUserQuestion` while the user was on another view (or the Mac was locked) froze silently: the per-project question card only reconciled on Agent-tab mount, so the prompt never rendered and nothing flagged it. Fix rides the existing `AppState.pending_answers` slot + `usePendingInteractions` store (no `~/.claude/` reading, no new persistence). New `list_pending_questions` command (+ unit-tested `collect_pending_questions` helper) snapshots all pending slots; `useStuckSessions` hook reconciles on app mount + window focus + visibilitychange and fires a one-time-per-prompt toast; surfaces are a `ProjectCard` "Waiting" pill, a tab-agnostic `StuckQuestionCallout` banner in `ProjectDetail` (reusing the extracted `AskUserQuestionCard`), and the toast. Answer path is the existing `agent_answer_question`. See decision of same date. **Follow-up:** the extracted `AskUserQuestionCard` is now shared by `Chat.tsx` and `ProjectDetail.tsx`, so the P6 a11y item for it (real `role="radio"`/`role="checkbox"` + keyboard nav) now covers both call sites at once.
- [x] **Longer retry budget + friendly exhausted-retry UX for gateway 529s** (2026-07-23) — A user's agent (pointed at `api.z.ai`) surfaced a raw `API Error: 529 [...] overloaded` bubble. The retry layer was already catching it (the user's exact string is the `retry.rs` test fixture) but gave up after 4 attempts / ~14s, and the exhausted path dumped the raw 529 in a red bubble with no retry affordance; separately the `ClaudeEvent::Retrying` event was emitted by the backend but **never typed or rendered in the frontend** (dead event — the agent looked frozen during the retry window). Three-part fix: (1) `MAX_ATTEMPTS` 4→9 + new `BACKOFF_TOTAL_BUDGET_MS = 300_000` (5 min) wall-clock ceiling enforced via a new pure `next_backoff(attempt, elapsed_ms)` (4 new tests); both `send_with_retry` and `send_streaming_with_retry` switched to it. (2) Added the missing `retrying` variant to the TS `ClaudeEvent` union + a `retrying` field on `StreamingState` (set on event, cleared on next result) + an inline amber "Gateway overloaded — retrying 2/9 in 4s…" row in `Chat`. (3) Client-side overload detection (`isOverloadError`, mirroring `retry::is_overloaded`) renders an amber bubble + `OverloadBanner` with a **Retry now** button that re-sends the last user prompt (recovered from `turns`) via `onSend` — which runs the full retry loop again. Transcript stays truthful (raw 529 still recorded); chose client-side detection over threading a new `error_kind` field through `AgentResponse`. See decision of same date. **Follow-up:** the non-streaming `send_with_retry` still doesn't emit `Retrying` (only the streaming variant does) — parity is a small follow-up if the non-streaming path becomes primary.

### P2 — Hardening (correctness, robustness, secret hygiene)
- [x] Move auth token out of plaintext `~/.config/loopdeck/config.yaml` into the OS keychain (macOS Keychain / Windows Credential Manager / Secret Service); `chmod 600` is the interim floor
- [x] Wrap blocking I/O in `spawn_blocking`: `list_projects`, `rescan_project`, `scan_directory`, `import_project` (`commands.rs`) — they previously ran sync walkdir + git subprocess spawning inside `async` Tauri commands; now offloaded to the blocking pool
- [x] Fix `Drop` blocking: `claude_session.rs:1183-1194` sleeps up to 7s reaping the child on a tokio worker thread — now reaped on the tokio blocking pool via a detached `spawn_blocking` task (preserving the graceful-EOF-then-SIGKILL sequence); `child` became `Option<Child>` so `Drop` can hand ownership to the reap task, with a synchronous no-runtime fallback so no zombie leaks
- [x] Resolve `claude` and `git` to absolute, vetted paths at spawn (`claude_session.rs`, `git.rs`) to defeat PATH hijack — new `binary` module skips non-absolute `$PATH` entries (closes the cwd-hijack vector), vets the executable bit, and pins the path via `OnceLock` for the process lifetime
- [x] Add a top-level React error boundary above `<App>` in `main.tsx` — pre-router crashes currently blank-screen with no recovery; new `RootErrorBoundary` class component wraps `<App>` (inside `React.StrictMode`) and renders a self-contained, LoopDeck-styled fallback with "Reload app" (guaranteed) + "Try again" (remounts the subtree via a keyed Fragment) and a collapsible error-details panel
- [x] Audit `expect()`/`unwrap()` under `panic = "abort"`; in particular `skills.rs:362` (a malformed user `settings.json` aborts the process on import) and `lib.rs:77`
- [ ] Add an absolute per-turn deadline or parked-slot expiry (`claude_session.rs`) — parked turns currently hold the per-project lock indefinitely
- [ ] Cap unbounded accumulation in `ResponseAccumulator` (`agents.rs:603-625`) — abort past a block/byte limit
- [ ] Cap log retention in `logging.rs` (daily rolling appender grows forever); confirm no `auth_token` is ever logged
- [ ] Replace `eprintln!` diagnostics in `project.rs:43,76,172` with `tracing::debug!`
- [ ] Strengthen `check_destructive_floor` further: prefix deny-list is now argv-analyzed, but `mv`/`cp` targeting `/`, `/etc`, `/usr`, `$HOME` root are still best-effort
- [ ] Reconcile the `claude_session.rs:218-224` doc comment ("default") with the actual `--permission-mode acceptEdits` arg

### P3 — Quality gates (CI, lint, tests)
- [ ] CI: `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `npm ci`, `tsc --noEmit`, `npm run build` on macOS/Linux/Windows matrices
- [ ] Frontend lint/format: ESLint + @typescript-eslint + eslint-plugin-react-hooks + Prettier; add `npm run lint`
- [ ] Rust lint/format: `rustfmt.toml` + `clippy.toml`; enforce `-D warnings`
- [ ] Frontend tests: vitest + @testing-library/react — prioritize `streamingState`, `groupLoopRuns`, the IPC wire-conversion in `tauri.ts:216-235`, and the streaming-turn lifecycle
- [ ] E2E: `@tauri-apps/driver` + WebdriverIO smoke covering scan → import → edit
- [ ] Dependabot + `cargo audit` + `npm audit` in CI; pin toolchain (`engines`, `packageManager`, `rust-toolchain.toml`)
- [ ] Type-narrow Tauri IPC at the boundary (zod/valibot); assert-never on the `ClaudeEvent.type` switch
- [ ] Pre-commit hook wiring the lint/format checks above
- [ ] SBOM / license check (`cargo bundle-licenses`) in CI

### P5 — Docs & policy
- [x] Add `LICENSE` (MIT) + `license` field in `package.json` and `Cargo.toml` — done 2026-07-23 as part of the Gate B "Add LICENSE and SECURITY.md" step
- [x] Add `SECURITY.md` documenting the agent threat model (subprocess spawn; **`--permission-mode default`**, not the stale `acceptEdits` this line assumed; destructive floor + `ConfirmChanges`) and vuln-reporting policy — done 2026-07-23 as part of the Gate B "Add LICENSE and SECURITY.md" step
- [ ] Add `CONTRIBUTING.md` (dev setup, commit style, branch model, PR checklist, `.loopdeck/` memory convention)
- [ ] Refresh README/CLAUDE — they claim "no agents" but the code has a full agent runtime with 29 IPC commands; fix the stale "30 tests" count (now 188)
- [ ] `git rm --cached .DS_Store .claude -r` (both tracked despite being in `.gitignore`)
- [ ] Move `docs/researchs/*.json` (~220KB captured provider payloads) to LFS or an external artifact store
- [ ] Add `.env.example` (DONE in Phase 1) — keep in sync as new env vars are introduced
- [ ] Architecture doc + user install guide: `docs/architecture.md`, `docs/USER_GUIDE.md`
- [ ] `CHANGELOG.md` (or generate via release tooling)

### P6 — UX polish (a11y, i18n, perf)
- [ ] A11y: replace the hand-rolled history dropdown with Radix `Popover`/`DropdownMenu` (`AgentPanel.tsx:852-905`) — gives focus trap, Esc, roving focus, ARIA
- [ ] A11y: give `AskUserQuestionCard`/`PermissionApprovalCard` real `role="radio"`/`role="checkbox"` + keyboard nav (`Chat.tsx:622-879`); currently glyphs (`●`/`○`) are the only state indicator
- [ ] A11y: `aria-live="polite"` on the streaming region; `aria-label` on the send button
- [ ] A11y: visually-hidden native `<input type="checkbox">` for GFM task-list items (`Markdown.tsx:73-87`)
- [ ] i18n scaffolding (react-intl/i18next) — every user-facing string is currently a hardcoded English literal
- [ ] Chat perf: `useMemo` `groupLoopRuns`, memoize completed `TurnBubble`, virtualize the transcript (`@tanstack/react-virtual`)
- [ ] Throttle `streamingState` delta writes (currently O(blocks) per token; use `useSyncExternalStore` selector + rAF)
- [ ] Auto-scroll: only stick-to-bottom when the user is already near the bottom (`Chat.tsx:921-925`)
- [ ] Persist only `selectedProject.path` in Zustand (not the full `ProjectEntry`) to avoid schema drift
- [ ] Move module-level mutable `lastSelectedPath` (`AgentRunner.tsx:30`) into the Zustand store
- [ ] Wire or remove the dead `cmdk` dependency; remove the misleading `⌘K` hint (`AppShell.tsx:152`)
- [ ] Fetch model presets from backend/config instead of hardcoding model IDs (`Settings.tsx:24-29`)
- [ ] Verify `lucide-react ^1.21.0` is the intended major (current major is much higher — likely a typo)
- [ ] Consolidate the three divergent relative-time formatters (`lib/time.ts`, `AgentPanel.tsx:53-70`, `Chat.tsx:96-102`)
- [ ] Add source-map strategy for prod error reporting; inject `__APP_VERSION__` via Vite `define`

## Parking Lot
- [ ] **Move agent control into LoopDeck app** — When LoopDeck can spawn/manage AI agents from within the app (not just the terminal), it should own all agent configuration: CLAUDE.md, skills, hooks, and memory conventions. The current `.claude/settings.local.json` hooks (PreToolUse dirty flag, Stop hook reminder) are temporary workarounds that only work in the Claude Code terminal context. Once LoopDeck controls the agent runtime, it can intelligently decide when to prompt for memory updates, apply project-specific instructions, and manage skills — without relying on external hook files.
- [ ] **macOS App Sandbox** — enable App Sandbox + scoped entitlements (user-selected files only) so a misbehaving agent is bounded by more than the OS user. Requires rethinking `current_dir(project_path)` access patterns.


## History

Completed loops have been archived to keep the live file small (~95% smaller).
Full history (28 completed loops, 2026-06-22 onward): [`loops-archive.md`](./loops-archive.md)
