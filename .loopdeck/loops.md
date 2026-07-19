# Loops

## Current

- **Started**: 2026-07-05
- **Goal**: Ship a hardened private alpha on one explicitly supported OS. The
  release gate is intentionally narrow: honest agent permissions, crash-safe
  critical state, bounded project/agent input, deterministic interruption
  recovery, basic CI, and a documented install/upgrade path. Public V0.1 adds
  signed artifacts and a small cross-boundary regression suite. Broader product
  maturity work remains tracked but does not block the alpha.
- **Status**: in_progress
- **Last completed**: 2026-07-19 — Cleared lint debt + tightened CI clippy to `-D warnings`. Resolved all 6 outstanding Clippy warnings: `dead_code` (test-only `parse_response` → `#[cfg(test)]`); `doc_lazy_continuation` (ranking-list paragraph break in `composer.rs`); `needless_borrows_for_generic_args` (`conversation.rs::reconcile_interrupted` `read_bounded_to_string`); `redundant_closure` (`logging.rs` → `or_else(platform_log_dir)`); `manual_map` (`memory.rs::parse_checklist` → `.map()`); `too_many_arguments` (`ConversationTurn::assistant` 9-field constructor → `#[allow]` with rationale; rejected a params-struct refactor — ~30 call sites mostly test literals, and the production sites in `commands/agent.rs` still read `response` after the call, so it can't take `AgentResponse` by value). `.github/workflows/ci.yml` clippy step is now `cargo clippy -- -D warnings` so new lint debt fails the gate. Verified: `cargo fmt --check` clean, `cargo clippy -- -D warnings` exits 0 (0 warnings), `cargo test` 313 passed / 0 failed (unchanged). Next unchecked gate step: "Alpha distribution contract". See decision of same date.

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
- [ ] **Alpha distribution contract:** name the one supported OS and document installation, upgrade/reinstall, rollback, prerequisites, and diagnostic-log location
- [ ] **Alpha smoke test:** manually verify import → start turn → approve/deny → interrupt → restart/recover on a packaged build

### Release Gate B — Public V0.1

- [ ] Sign/notarize artifacts for every OS claimed as supported; do not publish unsigned builds as production releases
- [ ] Define the release artifact pipeline and smoke-test installation plus upgrade/reinstall behavior
- [ ] Add focused frontend tests for streaming, approval, and interruption state transitions
- [ ] Add one automated cross-boundary smoke test covering import → agent approval → interrupt/recovery
- [ ] Add `LICENSE` and `SECURITY.md` with the agent/subprocess threat model and vulnerability-reporting path
- [ ] Provide user-accessible diagnostics and bounded log retention
- [ ] Persist only navigation identifiers/preferences in Zustand; reload project and run state from Rust

### Supporting backlog — not an alpha release gate

The detailed P2/P3/P5/P6 items below remain useful implementation notes. They
only block the private alpha when they directly satisfy a Gate A item above.
Do not expand the release definition by treating every unchecked item as a
prerequisite.

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
- [ ] Add `LICENSE` (MIT) + `license` field in `package.json` and `Cargo.toml`
- [ ] Add `SECURITY.md` documenting the agent threat model (subprocess spawn, `acceptEdits`, allow-by-default + destructive floor) and vuln-reporting policy
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

