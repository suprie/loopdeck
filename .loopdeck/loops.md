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
- **Last completed**: 2026-07-12 — Converted the hardening review into
  `docs/PRD-trust-boundary-hardening.md` and recalibrated production readiness:
  the private-alpha gate is separated from public-release and post-release
  maturity work so LoopDeck can harden the real trust boundaries without
  building mature-product infrastructure prematurely.

## Next Steps

### Release Gate A — Hardened private alpha

Source: [`docs/PRD-trust-boundary-hardening.md`](../docs/PRD-trust-boundary-hardening.md)

- [x] **Honest permission default:** ship `ConfirmChanges` first; remove generated `Edit(*)`, `Write(*)`, and broad build-runner rules; align Claude spawn settings, LoopDeck policy, approval UI, and regression tests
- [x] **Defer autonomous mode:** do not add per-project `AutonomousProject` configuration until the confirm-first path is proven usable; this is not an alpha blocker
- [x] **Crash-safe critical state:** add one shared atomic-write helper and use it for the registry, `project.yaml`, `loops.md`, PRDs, and generated Claude settings
- [x] **Recoverable registry:** keep one last-known-good backup and never overwrite a malformed primary registry with a fresh default
- [ ] **Central project boundary:** resolve every project-scoped IPC request through shared registered-root and contained-relative-path helpers; reject traversal and symlink escape
- [ ] **Bound untrusted work:** cap recursive scan depth/entries/time, file and NDJSON line sizes, `ResponseAccumulator` bytes/blocks, and parked approval/question duration
- [ ] **Minimal interruption recovery:** after restart or child failure, classify incomplete work as `interrupted`, clear stale busy/waiting state, and allow a new turn; persist a separate run record only if transcript-based recovery proves insufficient
- [ ] **Basic CI:** require `cargo fmt --check`, Clippy, `cargo test`, `npm ci`, and `npm run build`; start with the alpha's supported OS rather than a three-OS matrix
- [ ] **Clear current lint debt:** resolve existing Clippy failures before enabling `-D warnings`
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

### 2026-07-19 — Phase 2 — Crash-safe persistence + recoverable registry

- **Status**: completed
- **Completed**: 2026-07-19

Closes Gate A items 3 (Crash-safe critical state) and 4 (Recoverable
registry). Phase 2 of the trust-boundary hardening PRD made LoopDeck's
critical on-disk state survive process crashes and malformed data — no
database, no journal, just the write pattern the filesystem already gives
us for free.

**The bug it fixed.** Every critical write in the codebase used
`std::fs::write` (truncate-then-write): a crash, full disk, or OS-dropped
write between the open and the final byte left a partial file. The most
dangerous instance was the registry (`~/.config/loopdeck/config.yaml`): on
malformed YAML, `GlobalConfig::load()` returned `Err`, and `lib.rs` caught
it with `unwrap_or_else(|e| { let fresh = default(); fresh.save(); fresh })`
— silently overwriting the malformed primary with an empty default. That
turned recoverable corruption into silent data loss: the user's entire
project list vanished without a trace.

**Changes (5 commits, task-by-task).**
- `1ddccba` — `persist.rs` (NEW): one shared `atomic_write(path, contents)`
  primitive. Writes to a sibling temp file in the target's parent directory,
  flushes + fsyncs, then renames over the target. Same-directory rename is
  atomic on POSIX and atomic-with-replace on Windows for same-volume moves.
  The temp name includes the PID so two writers can't collide; a stale temp
  from a crashed prior run never conflicts. On any error the temp is removed
  and the original is left untouched. 8 unit tests cover create / overwrite
  / parent-dirs / no-temp-left / untouched-on-fail plus a `read_if_exists`
  helper.
- `6ca126e` — `config.rs`: `GlobalConfig::save_to_path` backs up the existing
  primary to `config.yaml.bak` (best-effort) before `atomic_write`-ing the
  new one. `load_from_path` runs a 4-step recovery: primary missing → fresh
  default; primary parses → load; primary malformed → try the backup and
  warn; both malformed → `Err`. The malformed primary is **never**
  overwritten. `lib.rs` startup turned the `unwrap_or_else` silent-overwrite
  into a hard `error!` + `exit(1)` so the user gets a structured log line
  and the file stays on disk for manual recovery. Test-friendly inner fns
  (`load_from_path` / `save_to_path`) take explicit paths so the 5 new
  recovery tests can redirect without touching production code paths.
  *Investigation note:* `serde_yaml` parses `:::not yaml:::` as a valid
  empty document — the malformed-test fixtures use `agent: [unclosed` (a
  rejected flow sequence) instead.
- `6ee84bc` — `memory.rs` + `epic.rs`: `persist::atomic_write` applied to
  every project-scoped markdown rewrite — `memory::toggle_loop_step` (loops.md
  checklist toggles), `memory::ensure_memory_files` (fresh decisions.md /
  loops.md bootstrap), `epic::toggle_epic_step` (loops.md Current section),
  `epic::toggle_prd_step` (PRD phase checklists), `epic::write_spec_file`
  (general PRD/epic markdown). `AppError`'s existing `#[from] std::io::Error`
  meant the `io::Result` from `atomic_write` converted via `?` with no
  adapter. Test-fixture writes in `#[cfg(test)]` stay on `std::fs::write`
  for speed (fsync would slow tests; fixtures own their temp dirs).
- `8b3d9c2` — `skills.rs`: `setup_hooks` writes the generated
  `.claude/settings.json` (the curated permissions allowlist + hook config)
  atomically. A crash here used to risk dropping the user's curated rules
  or leaving malformed JSON that claude refuses to load.
- `d697705` — `conversation.rs`: the whole-transcript rewrite path (promote
  archive to active — archives current then seeds fresh active.jsonl) now
  uses `atomic_write` so a crash can't break the next agent resume.
  `append_turn` now `flush()`es the OS page cache before returning. No
  `fsync` — that would tank streaming throughput; the PRD accepts line-
  atomic appends as sufficient. New test
  `load_tolerates_partial_final_line_after_crash` pins the recovery
  contract: a valid user/assistant pair followed by a truncated final line
  (what a crash mid-write leaves behind) loads the pair cleanly, skipping
  the partial. Complements the existing mid-file-malformed test.

**Design decisions.** Hand-rolled `persist` module over pulling in
`tempfile` or `atomicwrites` — the primitive is ~80 lines and full control
over the temp + fsync + rename sequence matters (e.g. dropping the file
handle before the rename for Windows compatibility). Test-friendly inner
fns (`load_from_path` / `save_to_path`) over an env-var path override —
production code paths stay untouched and the recovery tests are honest
about what they exercise. The `.bak` is overwritten on every save, so it's
at most 1× the primary size — no unbounded growth. `append_turn` skips
`fsync` deliberately: the PRD accepts line-atomic appends as sufficient
for transcripts and per-turn fsync would tank streaming throughput; the
`flush()` is enough to make an append visible to a same-process read
without waiting for OS writeback. `parse_turns` already skipped
unparseable lines via `filter_map` + warn, so read-side partial-line
tolerance was in place — the new test just pins the specific crash-during-
append scenario as a documented contract. `read_if_exists` helper lives in
`persist` with `#[allow(dead_code)]` until a production caller arrives.
See decisions.md ("Atomic writes via temp-file + fsync + same-dir rename;
last-known-good backup for the registry").

**Tradeoff accepted.** Every critical save now pays an `fsync`. In
practice these saves are infrequent (registry updates, loop toggles,
settings regen) — not per-token — so the latency is invisible. Transcript
*appends* deliberately skip fsync (only `flush()` to the page cache) to
keep streaming throughput reasonable.

**Verification.** `cargo fmt --check` clean; `cargo clippy --all-targets`
exit 0 — 0 new warnings (all 5 lib + 11 test warnings pre-existing at the
Phase 2 baseline, same 14 count); `cargo test --lib` 271 passed / 0 failed
/ 8 ignored (+13 vs Phase 1's 258: +7 `persist`, +5 registry recovery,
+1 partial-line tolerance); `npm run build` not re-run (backend-only
changes, frontend unaffected). The 8 ignored are the live
`claude`/keychain integration tests; recommend running them manually
before tagging the alpha. Manual smoke test (kill app mid-save, corrupt
registry on disk, restart) is the final confirmation the user runs.

**Files changed:** src-tauri/src/{persist.rs (new), lib.rs, config.rs,
memory.rs, epic.rs, skills.rs, conversation.rs}, .loopdeck/{loops.md,
decisions.md}.

### 2026-07-19 — Phase 1 — Honest permission contract (ConfirmChanges default)

- **Status**: completed
- **Completed**: 2026-07-19

Closes Gate A item 1 ("Honest permission default"). Phase 1 of the
trust-boundary hardening PRD made the agent permission policy explicit,
testable, and consistent across the four places it had previously been
contradicting itself. The four-arm `answer_control_request` flow in
`claude_session.rs` (AskUserQuestion → destructive floor →
`MANUAL_APPROVAL_TOOLS` interception → fallback policy) was already
confirm-first by construction; this phase fixed the three layers silently
bypassing it and renamed the misleading types so the contract is visible at
the type level.

**The four contradictions, pre-Phase-1.**

1. `claude_session.rs:227` ran `--permission-mode acceptEdits` while the
   comment directly above it said "Run in `default` permission mode".
   `acceptEdits` auto-approves Edit/Write/NotebookEdit inside Claude before
   any `control_request` reaches LoopDeck, so the `MANUAL_APPROVAL_TOOLS`
   entries for those three tools were dead code under that flag.
2. `skills.rs::setup_hooks` seeded a curated `.claude/settings.json` allow
   list that included `Edit(*)`, `Write(*)`, and broad build-runner rules
   (`Bash(cargo:*)`, `Bash(npm:*)`, `Bash(npx:*)`, `Bash(go:*)`,
   `Bash(pnpm:*)`, `Bash(yarn:*)`). These matched at the Claude layer
   before any `control_request` reached LoopDeck, silently auto-approving
   all file mutation and most build/test execution.
3. `permission.rs` types — `PolicyDefault::Allow/Deny`,
   `PermissionPolicy::allow_by_default()` — implied an
   auto-approve-everything posture that contradicted the actual confirm-first
   behavior (the manual-approval interception runs before the fallback
   `decide` is consulted, so under `Allow` the fallback only ever
   auto-allowed read-only tools).
4. The approval card in `Chat.tsx` had no effective-mode indicator, so a
   user couldn't tell what was gated vs. silent.

**Net effect pre-Phase-1:** all file edits + cargo/npm/go/pnpm/yarn command
execution were silently auto-approved. Only other Bash commands + WebFetch +
MCP reached the approval card. The UI implied more gating than existed.

**Changes (5 commits, task-by-task).**
- `32ce7a3` — `claude_session.rs`: flipped `--permission-mode acceptEdits` →
  `default` with an honest comment. One-line behavioral change, the
  highest-leverage fix in the phase: routes every un-ruled tool call through
  LoopDeck's policy instead of letting Claude auto-approve file edits.
- `beb7930` — `skills.rs::setup_hooks`: removed `Edit(*)`, `Write(*)`, and
  the six broad build-runner Bash rules from the curated `CURATED_ALLOW`
  array. Kept the narrow read-only rules (`Bash(ls:*)`, `Bash(git status:*)`,
  etc.). Comment block above the array now documents *what is deliberately
  not there and why* — a hostile repo controls its own scripts and build
  steps, so a broad allow rule is a privilege-escalation vector. The
  approval-card "Always allow" button remains the escape hatch for users to
  add narrow rules (e.g. `Bash(npm run test:*)`) to
  `.claude/settings.local.json` once they trust a project.
  `test_setup_hooks_writes_curated_allowlist` updated to assert the removed
  rules are absent.
- `fe9d175` — `permission.rs`: renamed `PolicyDefault` → `PermissionMode`
  (`Allow` → `ConfirmChanges`, `Deny` stays), `PermissionPolicy::allow_by_default`
  → `confirm_changes`, `with_default` → `with_mode`. Module-level doc
  rewritten to describe the actual confirm-changes posture. 10 call sites
  updated (commands.rs ×2, claude_session.rs test scaffolding ×8). The
  `deny_mode_*` tests and the `Deny` variant were kept: the ignored
  `test_session_deny_path_is_graceful` integration test uses them to verify
  the CLI recovers from a hard deny without hanging.
- `45e4bfc` — `permission.rs::tests`: new consolidated
  `confirm_changes_decision_matrix_documented` regression test pinning the
  full gating matrix in one readable place — read-only tools auto-allow,
  mutating tools (Edit/Write/NotebookEdit/WebFetch/Bash) clear the floor but
  gate on the UI card via `MANUAL_APPROVAL_TOOLS`, MCP tools always gate,
  and the destructive floor hard-denies regardless of mode. Pairs the
  `policy.decide` layer with the `requires_manual_approval` layer so a
  change to either is visible in one spot.
- `20909f6` — `src/components/shared/PermissionModeBadge.tsx` (NEW): a small
  "Confirm changes" badge with `ShieldCheck` icon (same icon as the approval
  card in Chat.tsx). Wired into `AgentPanel.tsx` toolbar (first child, left
  of Start button) and `AgentRunner.tsx` `PageHeader` actions Fragment.
  Designed as a single self-contained component so Phase 3's
  `AutonomousProject` mode has one file to update.

**Design decisions.** The four-arm `answer_control_request` flow was left
untouched — it was already correct. `AutonomousProject` was *not* added as a
type variant despite the plan initially suggesting it as future-proofing:
both match arms would return `Allow` under `ConfirmChanges` today (the
gating happens upstream in `MANUAL_APPROVAL_TOOLS` interception), so adding
`AutonomousProject` now would be a dead match arm — the textbook YAGNI
violation. It lands in Phase 3 alongside the path-containment helpers it
needs. The `Deny` variant was kept against the plan's suggestion to delete
it: reading the actual integration test showed `test_session_deny_path_is_graceful`
uses `PolicyDefault::Deny` to verify the CLI recovers from a hard deny, a
real safety property worth keeping verified. The curated allowlist keeps
its read-only Bash rules (`ls`, `cat`, `git status`, etc.) — they genuinely
reduce prompt fatigue without enabling mutation. See decisions.md
("ConfirmChanges as the default permission mode").

**Tradeoff accepted.** Flipping `acceptEdits` → `default` causes more
approval prompts: file edits the agent used to do silently now park on an
approval card. This is the intended behavior (PRD FR1) and the "Always
allow" button is the documented escape hatch. The PRD explicitly accepts
this tradeoff for the alpha.

**Verification.** `cargo fmt --check` clean; `cargo clippy --all-targets`
exit 0 (0 new warnings — all 5 lib + 11 test warnings pre-existing at Task 0
baseline); `cargo test --lib` 258 passed / 0 failed / 8 ignored (+1 from the
new decision-matrix test); `npx tsc --noEmit` clean; `npm run build` passes
(only the pre-existing >500kB chunk-size warning). The 8 ignored Rust tests
are the live `claude`/keychain integration tests; the spawn-flag change is
observable there but the offline tests assert the policy layer, which is
unaffected. Recommend running the ignored suite manually before tagging the
alpha.

**Files changed:** src-tauri/src/{claude_session.rs, skills.rs, permission.rs,
commands.rs}, src/components/{shared/PermissionModeBadge.tsx (new),
detail/AgentPanel.tsx, agent/AgentRunner.tsx}, .loopdeck/{loops.md, decisions.md}.

### 2026-07-19 — Transient gateway-error retry for agent turns (reconciliation)

- **Status**: completed
- **Completed**: 2026-07-19 (documented after the fact; the module shipped in
  the in-flight WIP between commits `7d4e860` and HEAD without a loops/decisions
  record — this entry closes that gap)

LoopDeck spawns the `claude` CLI as a subprocess and reads its turns as NDJSON.
A transient gateway failure — `529` overloaded, retryable on the provider side —
arrives as a normal `Ok(AgentResponse { is_error: true, result: "API Error: 529
… overloaded …" })`, not as a transport error or a process crash. Before this
work the caller's `is_error` check surfaced it as a hard turn failure, so a
single transient blip ended the loop run and the user had to re-prompt by hand.

**Changes.**
- `retry.rs` (NEW — was untracked, documented here): the retry policy.
  - `MAX_ATTEMPTS = 4` (1 initial + 3 retries), `BACKOFF_BASE_MS = 2_000`,
    `BACKOFF_FACTOR = 2`, `BACKOFF_CAP_MS = 30_000` — yielding ~2s, 4s, 8s
    backoffs before giving up.
  - `is_overloaded(result: &str) -> bool` — substring match for `529` or
    `overloaded`, case-insensitive. The `claude` CLI flattens gateway status
    into human-readable text; there is no structured status code on this side
    of the wire, so eligibility is a text match by necessity. Returns `false`
    for non-transient failures (401 auth, 400 bad request, "not logged in") —
    those won't fix themselves on retry and must surface immediately.
  - `backoff_ms(attempt: u32) -> Option<u64>` — 0-based attempt index → wait
    before the next try, exponential with the cap. `None` when `attempt + 1 >=
    MAX_ATTEMPTS` (no retry slot left), which the callers use as the terminal
    signal. `saturating_mul`/`saturating_pow` guard the 32-bit multiply long
    before the cap binds; `.min(BACKOFF_CAP_MS)` is what actually enforces it.
  - 5 unit tests covering the real captured 529 string, variants, non-transient
    negatives, the backoff progression, and the cap.
- `agents.rs`: new `ClaudeEvent::Retrying { attempt, max_attempts, backoff_ms,
  error }` variant. Emitted on the streaming path *between* a failed attempt
  and its retry so the UI can show "Retrying 2/4 in 4s…" instead of seeing a
  terminal `Result{is_error:true}` silently followed by a second `Result`. The
  final `Result` (success or terminal failure) stays authoritative.
- `commands.rs`: two retry wrappers, `send_with_retry` and
  `send_streaming_with_retry`, wrapping `ClaudeSession::send_message` and
  `send_message_streaming`. Each loops until a non-retryable outcome (success,
  non-overload error, or `MAX_ATTEMPTS` exhausted), then returns the final
  `AgentResponse` — which may still carry `is_error: true`; the caller's
  `is_error` check decides whether to propagate as `Err`. Both log at `warn!`
  on every retry and on exhaustion. The streaming wrapper emits
  `ClaudeEvent::Retrying` with the upcoming 1-based attempt number
  (`attempt + 2`) between attempts.

**Design decisions.** Transcript recording stays *out* of the retry wrappers:
the pipeline helpers (`send_and_record` / `send_and_record_streaming`) record
the user turn once before sending and the final assistant turn once after, so a
retried turn appears as a single exchange in the transcript, not N — the user's
intent and the eventual outcome are what's durable, not the transient blips.
Backoff uses `tokio::time::sleep` (the `time` feature was already enabled).
Non-transient errors return immediately rather than burning the retry budget —
retrying a 401 or a 400 only delays the user's feedback. The substring match
is deliberately loose on wording (`overloaded`) and tight on status (`529`) so
it survives minor CLI/gateway phrasing changes without matching unrelated
errors; the test suite pins representative positive and negative strings. See
decisions.md ("Transient gateway-error retry for agent turns").

**Verification.** `cargo fmt --check` clean; `cargo clippy --all-targets` exit
0 — 0 new warnings introduced (all 5 lib + 6 test warnings pre-existing at
HEAD, none in `retry.rs` or the `Retrying` variant); `cargo test --lib` 257
passed / 0 failed / 8 ignored (+5 from `retry::tests`); `npm run build` passes
(only the pre-existing >500kB chunk-size warning). The 8 ignored are the live
`claude`/keychain integration tests; retry behavior against a real overloaded
gateway is not unit-testable from this side of the CLI, so the contract is
pinned by the 5 substring/backoff tests plus the captured real-world 529
string in `matches_the_real_529_message`.

Files changed: src-tauri/src/{retry.rs (new), agents.rs, commands.rs},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-10 — Panic/abort audit + hardening (P2 robustness)

- **Status**: completed
- **Completed**: 2026-07-10

Under `[profile.release] panic = "abort"`, any panic is a process abort — so
any `expect()`/`unwrap()` reachable from user or untrusted input during
normal operation is a robustness defect (one malformed file kills the whole
desktop app). The audit item named two spots; the work swept the whole crate.

**Audit method.** Grep'd every `unwrap()`/`expect()`/`panic!`/`unreachable!`
in `src-tauri/src`, mapped each file's `#[cfg(test)]` boundary, and
classified every *production* (pre-test) hit as either user-input-reachable
(defect) or a provable programmer invariant (acceptable). All `#[cfg(test)]`
panics and all `.unwrap_or_else(...)` sites with graceful fallbacks were
already safe.

**Production panic surface, classified.**
- `skills.rs` `find_or_create_matcher_group` —
  `.expect("hook event must be an array")`. **User-input-reachable → FIXED.**
- `lib.rs` `.run(...).expect("error while running tauri application")` —
  startup-only, no recovery path → **hardened to logged exit.**
- `permission.rs` `split_stages` (×4) — `.expect("stages non-empty")`;
  `stages` is seeded `vec![String::new()]` and only ever `.push()`ed →
  provably never empty. **Safe invariant — left as-is.**
- `commands.rs:1018` `single.into_iter().next().unwrap()` — `single` is a
  1-element vec and `derive_run_states` only mutates entries in place →
  provably non-empty. **Safe invariant — left as-is.**
- `memory.rs:174` `checklist_prefix_len(...).unwrap()` — re-derives a prefix
  whose `.is_some()` was already checked in the preceding search loop.
  **Safe invariant — left as-is.**

**Changes.**
- `skills.rs`: `find_or_create_matcher_group` now coerces a non-array hook
  event to an empty array instead of panicking. Reachable from `setup_hooks`
  (on the import path) when a user's `.claude/settings.json` holds a hook
  event keyed to a non-array value (string/number/object — hand-edited or
  from a different tool). The prior guard only created the array when the key
  was *missing*; a present-but-non-array value flowed straight into the
  `.expect()`. The coercion mirrors the lenient JSON-parse fallback already
  at the top of `setup_hooks` (malformed JSON → `{}`): treat malformed user
  config as "no existing hooks" and proceed. The surviving `.expect()` is now
  provably unreachable (coerced immediately above) — an invariant, not a
  user-input abort. New test `test_setup_hooks_survives_malformed_hook_events`
  feeds `Stop: "not-an-array"` + `PreToolUse: 42` and asserts both get coerced
  to populated arrays.
- `lib.rs`: `.run(tauri::generate_context!()).expect(...)` → `if let Err(e) =
  ...run(...) { tracing::error!(...); std::process::exit(1); }`. `run()` only
  errors on an unrecoverable startup failure (WebView init, context
  generation, plugin init) — terminating is correct either way — but the
  explicit form removes the panic/abort source under `panic = "abort"` and
  lands a structured log line (tracing is initialized at the top of `run()`)
  instead of a raw panic message, so a user can find *why* the app won't
  start. `std::process::exit` doesn't run destructors, but neither does an
  abort, and there's nothing meaningful to drop at that point.

**Design decisions.** The three "safe invariant" sites were left as-is rather
than rewritten: under abort the audit's concern is panics *reachable from bad
input*, not every `.unwrap()` in the codebase, and converting provable
invariants to defensive `match`/`if let` would be churn with no robustness
gain. The `skills.rs` fix is lenient (coerce + proceed) rather than returning
an `AppError`, because `setup_hooks` already degrades leniently on a
malformed `settings.json` elsewhere and erroring the whole import on one odd
hook field would be worse than overwriting it. See decisions.md ("Lenient
coercion for malformed user hook events; logged exit for unrecoverable Tauri
startup").

**Verification.** `cargo check --lib` clean (1 pre-existing `parse_response`
dead-code warning in `agents.rs`); `cargo clippy --lib` 0 new warnings (all
5 pre-existing); `cargo fmt` applied to the two changed files; `cargo test
--lib` 257 passed / 0 failed / 8 ignored (was 256; +1
`test_setup_hooks_survives_malformed_hook_events`). The 8 ignored are the
live `claude`/keychain integration tests (need network +
`LOOPDECK_TEST_AUTH_TOKEN`).

Files changed: src-tauri/src/{skills.rs, lib.rs},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-10 — Top-level React error boundary (P2 robustness)

- **Status**: completed
- **Completed**: 2026-07-10

A render-time crash anywhere in the app tree blanked the window with no
recovery path. The router's `errorComponent` (`router.tsx` `ErrorComponent`)
only catches errors thrown *inside* a route component — anything that blows up
before or around the router (`ThemeProvider`, `RouterProvider`, `Toaster`, the
root `App`'s own render) fell through to React's default uncaught-error
behavior: a blank white screen.

**Changes.**
- `RootErrorBoundary.tsx` (NEW — `src/components/shared/RootErrorBoundary.tsx`):
  a class component implementing the canonical React error-boundary contract
  (`static getDerivedStateFromError` + `componentDidCatch` + a render-time
  fallback). Mounted above `<App>` so it spans the whole tree the router can't.
  State is `{ error, resetKey }`.
- `main.tsx`: wraps `<App />` in `<RootErrorBoundary>` inside `React.StrictMode`.
  The boundary sits closest to the failing tree (StrictMode doesn't catch
  errors, so its position relative to the boundary doesn't matter).

**Recovery model (two buttons).**
- **Reload app** — `window.location.reload()`. Guaranteed recovery. LoopDeck is
  offline-first, so on-disk data (`.loopdeck/`, `config.yaml`, the keychain
  auth token) survives a full reload untouched; the copy explains this.
- **Try again** — `setState({ error: null, resetKey: prev + 1 })`. The bumped
  `resetKey` is applied as a `Fragment` `key` around the children, forcing React
  to unmount + remount the whole app subtree (dropping transient React state;
  Zustand persists to storage so it rehydrates). A *deterministic* crash will
  re-trip the boundary immediately — leaving the user no worse off and pointed
  at Reload. Best-effort, not a guarantee.

**Design decisions.** The fallback is deliberately self-contained — no Tauri,
router, theme, or store imports — so it renders even when the provider it
normally wraps is the thing that crashed. It relies only on the base OKLCH
tokens declared in `:root` (`styles.css`), which are present without
`ThemeProvider` (ThemeProvider only toggles the `.dark` class; base tokens
exist unconditionally). Styling matches the nearest sibling UI — the router's
`ErrorComponent` / `NotFoundComponent` (centered max-w-md card, same primary +
secondary button classes). `cn()` was dropped for the single unconditional
className, matching `ErrorComponent`'s plain-string style. A collapsible
error-details panel shows `error.message` + `stack` for dev diagnostics
(future source-map / `__APP_VERSION__` reporting is a P6 item). React error
boundaries catch render / lifecycle / constructor errors only — async,
event-handler, and `useEffect`-callback errors are out of scope and already
surface through `appStore.error` banner + toasts. See decisions.md ("Top-level
React error boundary above `<App>`").

**Verification.** `npx tsc --noEmit` clean; `npm run build` passes (only the
pre-existing >500kB chunk-size warning, unrelated). No frontend test harness
exists yet (the P3 vitest task), so the React error-boundary contract was
verified by construction — the change is a passthrough when healthy (renders
children unchanged), so it cannot regress existing behavior.

Files changed: src/{main.tsx, components/shared/RootErrorBoundary.tsx (new)},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-10 — Absolute, vetted binary resolution for `claude` + `git` (P2 security)

- **Status**: completed
- **Completed**: 2026-07-10

Both subprocess spawns handed a bare name to the OS PATH search at spawn time
(`Command::new("claude")` / `Command::new("git")`). The production-readiness
audit flagged this as "over-broad capabilities + PATH-resolved binaries": with a
`.` or empty `$PATH` entry present, the bare-name search resolves against the
process cwd — which is a user-selected project for `claude` (where the agent
auth token is also injected into the child env) and a scanned repo for `git`.
A project shipping a `claude`/`git` script could otherwise run that script under
LoopDeck's privileges.

**Changes.**
- `binary.rs` (NEW): `resolve_command(name)` walks `$PATH` via
  `std::env::split_paths` and returns the first absolute, executable match.
  Three defenses: (1) **skips every non-absolute component** — an empty string
  or `.`/relative dir means "the cwd", which is the hijack vector this closes;
  (2) **vets executability** — `metadata` follows symlinks, requires a regular
  file, and (Unix) a set execute bit (`mode & 0o111`); (3) **pins the result in
  a `OnceLock`** so a later `$PATH` mutation by a sibling/child process cannot
  redirect subsequent spawns. `git()` / `claude()` are the two cached accessors;
  a failed resolution leaves the cell empty and re-resolves next call (so a
  mid-session install is picked up). Windows probes `exe`/`bat`/`cmd`/`com`
  (PATHEXT mirroring); non-`NotFound` stat errors are logged at debug and
  treated as "not a match" so one unreadable PATH entry can't abort the search.
  7 unit tests cover the hijack defense, executability gating, and the
  mixed-relative/absolute PATH case deterministically (temp dirs + `chmod`).
- `claude_session.rs`: `spawn` resolves `claude` via `crate::binary::claude()`
  before `Command::new`; resolution failure is a hard `AppError::Agent` (no
  claude ⇒ no agent). The resolved path is logged at INFO so a hostile `$PATH`
  (a different claude than the user expects) is visible.
- `git.rs`: new `git_command(repo_path)` resolves `git` via `crate::binary::git()`
  and pins the repo with `git -C <repo_path>` instead of `current_dir(repo_path)`
  + bare name. All six production git helpers route through it. Resolution
  failure returns `None`, so callers see "no git info" — matching the prior
  behavior of a failed spawn (rather than erroring the whole `check_git_info`).
- `lib.rs`: `mod binary;`.

**Deliberately left bare.** The `Command::new("git")` calls inside `git.rs`
`#[cfg(test)]` (test repo setup) and the `Command::new("claude")` calls in
`agents.rs` `apply_agent_config` tests — the former spawn in temp dirs the test
owns, the latter never spawn at all (they build a `Command` only to inspect the
env vars via `get_envs()`). Neither touches a user-selected/untrusted directory.
`call_agents` survives only as a stale doc reference; the single-shot spawn path
no longer exists, so there's no second production `claude` spawn to fix.

**Design decisions.** Failure semantics split by consumer: claude must exist to
function (hard error), git is advisory metadata (soft `None`). `OnceLock` over
re-resolving-every-time trades freshness for pinning — a once-good binary stays
good, and a once-missing one is retried until found. What this *cannot* prevent
is documented in the module: a compromised `$PATH` where a legitimate earlier
entry wins by design, and the GUI-launch minimal-PATH blind spot (no Homebrew /
npm global dirs) — discovering common install dirs is a separate follow-up, out
of scope for the hijack fix. See decisions.md ("Resolve `claude`/`git` to
absolute, vetted paths at spawn").

**Verification.** `cargo check --lib` clean (1 pre-existing dead-code warning in
`agents.rs`); `cargo fmt --check` clean for the changed files (only `commands.rs`
— the unrelated retry work — shows diffs); `cargo clippy --lib` 0 new warnings
(all 5 pre-existing, none in `binary.rs`/`git.rs`/`claude_session.rs`); `cargo
test --lib` 256 passed / 0 failed / 8 ignored (was 244; +7 from `binary::tests`,
rest from concurrent work). The 8 ignored are the live `claude` integration
tests (need `LOOPDECK_TEST_AUTH_TOKEN` + network); the spawn mechanics they
exercise are unchanged.

Files changed: src-tauri/src/{binary.rs (new), git.rs, claude_session.rs, lib.rs},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-10 — `ClaudeSession::drop` reap moved off the tokio worker (P2 robustness)

- **Status**: completed
- **Completed**: 2026-07-10

`ClaudeSession::drop` reaped the child with a synchronous `poll_reap` loop
(`try_wait()` paced by `thread::sleep`, up to 7s: 5s graceful EOF window + 2s
after `start_kill`). Because `Drop` can't `.await`, the sleep was the
recommended sync-reap pattern — but `ClaudeSession` lives behind
`Arc<tokio::sync::Mutex<…>>` and is dropped from inside async Tauri commands,
so that 7s of `thread::sleep` ran on a tokio worker thread, stalling every
other async task on the runtime (the UI, plus concurrent IPC). The
`Drop`-flavour twin of the blocking-on-worker anti-pattern the
`spawn_blocking` command fix had just closed.

**Changes (`claude_session.rs`).**
- `child: Child` → `child: Option<Child>` so `Drop` can `take()` ownership and
  hand it to a reap task without touching `self` (being torn down). The field
  is only touched in `spawn` and `Drop`, so the change is contained.
- New free fn `reap_child(child: Child, stderr_drain: Option<JoinHandle<()>>)`
  holding the graceful-then-forceful sequence: `poll_reap(5s)` → if not reaped,
  `start_kill()` + `poll_reap(2s)` → then abort the stderr-drain task.
  Extracted from the old inline `Drop` body; `poll_reap` itself is unchanged.
- `Drop` now closes stdin, takes `child` + `stderr_drain`, and dispatches:
  - Runtime present (`Handle::try_current()` → `Ok`): fire-and-forget
    `handle.spawn_blocking(move || reap_child(...))`, detached via `drop(...)`
    (dropping a `spawn_blocking` `JoinHandle` does NOT cancel the task — it runs
    to completion; distinct from `Child::kill()`, whose future must be awaited
    to send the signal — the checkpoint-4 bug).
  - No runtime (teardown / drop from a non-runtime thread): call `reap_child`
    synchronously. Blocking is fine there (no async worker to stall) and it
    prevents a zombie.

**Design decisions.** `spawn_blocking` was chosen over `tokio::spawn` +
`child.wait().await` to keep the bounded graceful-then-forceful reap (claude
gets a chance to flush its `--resume` session state before SIGKILL) rather
than immediate-kill-and-detach. The stderr-drain task is kept alive
*throughout* the reap and aborted only after the child is gone, preserving the
original ordering so a verbose child can't fill its stderr pipe buffer and
block on exit during the graceful window. The `try_current()` guard means the
common case (drop inside an async command) never blocks a worker, while the
edge case (drop with no runtime) still reaps instead of leaking. No
caller-observable behavior change; the live integration tests assert session
semantics, not which thread reaps. See decisions.md ("Reap claude child off
the tokio worker in `Drop` via `spawn_blocking`").

**Verification.** `cargo check --lib` clean (1 pre-existing dead-code warning
in `agents.rs`); `cargo fmt --check` clean; `cargo clippy --all-targets`
introduces 0 new warnings (the `let_underscore_future` lint on the detached
`spawn_blocking` handle was resolved with `drop(...)` — clippy's recommended
fix for an intentional detach, and not the real unawaited-`kill()`-future bug
it flagged in checkpoint 4); `cargo test --lib` 244 passed / 0 failed / 8
ignored. The 8 ignored are the live `claude` integration tests (need
`LOOPDECK_TEST_AUTH_TOKEN` + network); not re-run here, but the reap behavior
is unchanged so they should still pass.

Files changed: src-tauri/src/claude_session.rs,
.loopdeck/{loops.md, decisions.md}.

### 2026-07-10 — Blocking I/O offloaded to `spawn_blocking` (P2 robustness)

- **Status**: completed
- **Completed**: 2026-07-10

Four `async` Tauri commands were doing real blocking work on the tokio worker
thread — recursive `walkdir`, per-repo `git` subprocess spawns, and file reads.
A scan of a large directory tree or a `list_projects` over many repos parked the
worker for seconds, stalling every other IPC command sharing that thread (the UI
froze for the duration). The `Mutex<GlobalConfig>` lock was held *across* that
work too, so concurrent config access blocked as well.

**Changes (`commands.rs`).**
- New private `blocking_task_failed(JoinError) -> AppError` helper + new
  `AppError::BlockingTask(String)` variant (with serialize `kind` arm) so a join
  failure (task panic/cancel) crosses the IPC boundary instead of leaking a raw
  `tokio::task::JoinError`.
- `scan_directory`: `scanner::scan_directory` (walkdir + per-repo git) moved
  inside `spawn_blocking`. The config lock is acquired only briefly — once before
  (to read `scan_depth`) and once after (to cross-reference `has_loopdeck`).
- `import_project`: the early "already registered" return stays under a brief
  lock; the heavy bootstrapping — `quick_scan_directory` + `bootstrap_project` +
  `git::check_git_info` + `read_current_loop` — runs in `spawn_blocking`,
  returning `(ProjectMeta, GitInfo, Option<String>)`. Registry add + `save()`
  happen under a brief lock after.
- `list_projects`: snapshots project paths under a brief lock, refreshes git
  info + current loop per project on the blocking pool keyed by path, then
  applies + saves under a brief lock. Keying by path keeps the apply aligned
  even if the registry changed between snapshot and apply. `derive_run_states`
  now runs outside the config lock.
- `rescan_project`: resolves the target (registered + path-exists) under a
  brief lock, runs `git::check_git_info` in `spawn_blocking`, applies + saves
  under a brief lock. Preserved the prior quirk of not refreshing
  `last_commit_message` (out of scope to change).

**Design decisions.** The multi-second walkdir/git work was the problem;
`config.save()` (a brief atomic file write) is intentionally kept on the worker
under the lock. Command signatures and return types are unchanged, so no
frontend edits were needed (the frontend only switches on `kind === "conflict"`,
unaffected by the new variant). See decisions.md ("Offload blocking I/O in Tauri
commands to `spawn_blocking`").

**Verification.** `cargo check --lib` clean (1 pre-existing dead-code warning);
`cargo fmt --check` clean for the changed files (pre-existing `secrets.rs`
diffs untouched); `cargo clippy --all-targets` introduces 0 new warnings (all
warnings pre-existing); `cargo test --lib` 242 passed / 0 failed / 8 ignored.

Files changed: src-tauri/src/{commands.rs, error.rs},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-10 — Auth token moved to OS keychain (P2 secret hygiene)

- **Status**: completed
- **Completed**: 2026-07-10

Moved the agent auth token out of plaintext `~/.config/loopdeck/config.yaml`
into the platform-native credential store, and hardened the config file's
permissions as a defense-in-depth floor.

**Backend.**
- `secrets.rs` (already scaffolded): `load_auth_token` / `store_auth_token` /
  `delete_auth_token` over the `keyring` crate (macOS Keychain / Windows
  Credential Manager / Linux Secret Service). Failures degrade gracefully — a
  missing token is `Ok(None)`; an unrecoverable backend is `AppError::Config`.
- `config.rs`: `AgentConfig` gained a `has_auth_token: bool` presence flag
  (`#[serde(default, skip_serializing_if = "is_false")]`) — populated only on
  the `get_agent_config` read path so the UI can show a "token stored"
  affordance; never persisted to YAML. `GlobalConfig::save()` now applies a
  `0600` owner-only floor on Unix (`restrict_file_perms`). New
  `migrate_auth_token_to_keychain()` moves a plaintext token to the keychain
  and scrubs it from the in-memory config (`Ok(true)` if it moved something,
  `Ok(false)` for none/empty, `Err` if the keychain rejected a real token —
  then the token is put back so it's never lost).
- `commands.rs`: `set_agent_config` stores a newly-typed token in the keychain
  and scrubs it from the persisted YAML; an empty/`None` token means "leave the
  existing keychain token untouched" (the UI never receives the plaintext back,
  so an unchanged field shows up empty). `get_agent_config` never returns the
  plaintext token — only `has_auth_token`. New `resolve_agent_config` helper
  injects the keychain token at spawn time into a local `AgentConfig` (the
  token lives only for the spawn call, never on `Mutex<GlobalConfig>`), used by
  both `with_session` and the reset-spawn path. New `clear_auth_token` command.
- `lib.rs`: startup migration — `migrate_auth_token_to_keychain` + `save` so a
  plaintext token left by a prior version is moved on first launch and the file
  is (re)tightened to `0600`. If the keychain is unavailable the token stays in
  the `0600` file as the interim floor.
- Drive-by: removed a dead `use tokio::process::Command;` in `agents.rs` left
  by the prior `CommandEnv` refactor.

**Frontend.**
- `types/index.ts`: `AgentConfig.has_auth_token?: boolean`.
- `lib/tauri.ts`: `clearAuthToken()` IPC wrapper.
- `Settings.tsx`: the auth-token field no longer pre-fills the plaintext token
  (it isn't returned over IPC); a "Token stored in OS keychain" badge with a
  "Clear stored token" button appears when a token is set and the field is
  empty; saving a new token clears the field and flips to the stored badge;
  the hint now says the token is stored in the OS keychain, not config.yaml.

**Design decisions.** The plaintext token never crosses IPC to the renderer —
only a presence flag does. The token is resolved from the keychain at spawn
time into a local value, not held on long-lived app state. Keychain
unavailability falls back to the `0600` plaintext file rather than dropping the
token. See decisions.md ("Auth token stored in OS keychain, not config.yaml").

**Verification.** `cargo test --lib` 242 passed / 0 failed / 8 ignored (+5 new
config tests for the presence flag + migration no-op paths); live
`cargo test --lib secrets -- --ignored` round-trip passes against the real
macOS Keychain; `cargo clippy --all-targets` clean for new code; `tsc --noEmit`
clean; `npm run build` passes.

Files changed: src-tauri/src/{config.rs, commands.rs, lib.rs, agents.rs,
claude_session.rs, secrets.rs}, src/{types/index.ts, lib/tauri.ts,
components/settings/Settings.tsx}, .env.example, .loopdeck/{loops.md, decisions.md}.

### 2026-07-05 — Phase 1: Stop the bleeding (security)

- **Status**: completed
- **Completed**: 2026-07-05

Closed the critical/high security findings from the production-readiness audit.
Reframing documented for future readers: `permission.rs` already parks
`Bash`/`Edit`/`Write`/`NotebookEdit`/`WebFetch`/MCP on a manual-approval card
(`MANUAL_APPROVAL_TOOLS`), so the user *is* in the loop for mutating tools; the
"allow-by-default" only governs read-only tools. The destructive floor is the
backstop for when a user clicks "Allow" without reading — that's what got
strengthened, rather than flipping the whole posture.

**Changes.**

- **Leaked API key removed from source.** `sk-64a72…` was hardcoded in
  `claude_session.rs` test_config and a dead commented block in `agents.rs`.
  `test_config()` now reads `LOOPDECK_TEST_AUTH_TOKEN` from env (also
  `LOOPDECK_TEST_BASE_URL`/`LOOPDECK_TEST_MODEL`); the dead block was deleted.
  History scrub intentionally skipped (rotated literal is dead). New
  `.env.example` documents all runtime + test env vars.
- **Strict CSP** (`tauri.conf.json`). Was `null`; now `default-src 'self' ipc:
  http://ipc.localhost`, `script-src 'self'` (no unsafe-inline/eval), only
  `style-src 'unsafe-inline'` (Tailwind v4 + Radix need it). img/font/data
  allowlisted minimally.
- **Shell injection fixed** (`commands.rs`). New `resolve_dir_arg()` canonicalizes
  and rejects non-directories — used by both `open_in_finder` and
  `open_in_terminal` (blocks the macOS `open "x-apple-…"` handler trick).
  macOS terminal launchers no longer interpolate the path into AppleScript —
  it's passed as a separate `osascript … <path>` argv with `on run argv` +
  `quoted form of`. Windows dropped `cmd /C "cd /d {path}"` for `cmd /K` +
  `current_dir()`. Linux was already safe.
- **Destructive floor strengthened** (`permission.rs`). Was a 5-rule prefix list
  trivially bypassed (`rm -r -f`, `find -delete`, `ls; rm -rf`, etc.). Now: lex
  with `shell-words`, quote-aware stage split on `| ; && ||`, walk every stage,
  peel privilege wrappers (`sudo`/`command`/`exec`/`eval`/…), inspect argv[0] +
  flags (case-insensitive, short-flag bundling aware). Catches `rm` with
  recursive+force in any order/form, `find -delete`/`-exec rm`, `dd`, `mkfs*`,
  `chmod/chown -R`, all force-push variants including `--force-with-lease`,
  `curl|sh` pipe-to-shell, smuggled pipeline stages, and wrappers. Falls back
  to legacy prefix rules if parse fails. +13 new tests, all 30 floor tests pass.
- **Capabilities scoped** (`capabilities/default.json`). `shell:default` →
  `shell:allow-open` (the only shell function the frontend uses is `open()` in
  `Markdown.tsx`).
- **Markdown href allowlist** (`Markdown.tsx`). The `a` override handed any
  `href` to Tauri `open()`; now only `http:`/`https:`/`mailto:` get through,
  others drop `href` entirely. Belt-and-braces alongside the CSP.

**Verification.** `cargo test --lib` 188 passed / 0 failed / 7 ignored (was 175
before; +13 new floor tests). `cargo clippy --all-targets` 0 warnings. `cargo fmt`
applied. `npm run build` passes (tsc + vite).

**Outstanding from this loop (carried to P2-P6).** Code signing / notarization /
updater (P2); plaintext token → keychain, `spawn_blocking`, `Drop` blocking,
absolute-path binary resolution, top-level error boundary (P3). CI, lint, frontend
tests (P4). LICENSE / SECURITY.md / CONTRIBUTING / README refresh (P5).

Files changed: src-tauri/src/{permission.rs, commands.rs, claude_session.rs,
agents.rs, Cargo.toml}, src-tauri/{tauri.conf.json, capabilities/default.json},
src/components/shared/Markdown.tsx, .env.example (new), .loopdeck/loops.md.

### 2026-07-05 — Production readiness audit

- **Status**: completed
- **Completed**: 2026-07-05

Three-pronged read of the codebase (Rust backend, React frontend, ops/tooling)
to enumerate what's needed before production. Verdict: not production-ready —
solid foundation (typed IPC, good Rust test coverage, navigation-stable
streaming stores) but real blockers in security, distribution, and quality
gates. Findings fed directly into the Phase 1-6 next-steps list above.

Top blockers surfaced: leaked API key in git history; no CSP; shell injection
in `open_in_terminal`; over-broad capabilities + PATH-resolved binaries; no
code signing / notarization / updater; no CI; no LICENSE/SECURITY/CONTRIBUTING;
zero frontend tests; no markdown sanitization; A11y failures on hand-rolled
dropdowns and question/approval cards.

### 2026-07-03 — TanStack Router (replace Zustand view switching)

- **Status**: completed
- **Completed**: 2026-07-03

Replaced the Zustand `currentView` + conditional rendering pattern with
`@tanstack/react-router` v1 using memory history (no browser URL bar in Tauri).
Views are now proper routes with type-safe navigation.

**`router.tsx`** (NEW — `src/router.tsx`, 200 lines):
- `AppShellLayout` — root route component with sidebar nav, error banner, and
  `<Outlet />` for child route content. Nav items use `<Link>` components;
  active-state detection uses `useRouterState().location.pathname`.
- Route tree: `/` (Dashboard), `/activity`, `/agent`, `/decisions`, `/loops`,
  `/settings`, `/import`, `/project/$projectPath` (URL-encoded filesystem path).
- `createMemoryHistory({ initialEntries: ["/"] })` — in-memory routing for
  the Tauri desktop environment.
- Re-exports `RouterProvider`, `Outlet`, `useNavigate`, `useParams`, `Link`
  for convenience.

**Store changes** (`appStore.ts`):
- Removed `currentView` and `setCurrentView` from the Zustand store — navigation
  is now the router's responsibility.
- `setSelectedProject` simplified: no longer sets `currentView` to `"detail"`.
- Persisted state (`loopdeck-nav`) reduced to `selectedProject` + `detailTab`.

**Hook changes** (`useProjects.ts`):
- `scanFolder` → navigates to `/import` after scan.
- `importRepo` → navigates to `/` after import.
- `removeProject` → navigates to `/` after removal.
- All use `useNavigate()` instead of the old `setCurrentView`.

**View component changes:**
- `Dashboard.tsx`: `handleSelect` and `handleStart` now call `navigate({ to: "/project/$projectPath", params: { projectPath: encodeURIComponent(path) } })` instead of relying on `setSelectedProject` to change views.
- `ProjectDetail.tsx`: reads `projectPath` from `useParams()`, decodes with `decodeURIComponent`, syncs `selectedProject` via `useEffect`. Back button uses `navigate({ to: "/" })`.
- `ImportFlow.tsx`: back button uses `navigate({ to: "/" })`.
- `AppShell.tsx`: reduced to only `PageHeader` export (the layout moved to `router.tsx`'s `AppShellLayout`).

**Design decisions:**
- **Memory history over browser history** — Tauri has no URL bar, so
  `createMemoryHistory` is the natural fit. Routes are purely internal.
- **URL-encoded project paths** — filesystem paths like `/Users/foo/bar` are
  URI-encoded so they travel as a single route segment (`/project/$projectPath`).
  `encodeURIComponent`/`decodeURIComponent` at the navigation/cosumption boundary.
- **Zustand for data, Router for navigation** — the store still owns
  `selectedProject`, `detailTab`, `pendingAgentStart`, and the project list.
  The router owns the current location. No more store-driven view switching.
- **Persisted selection reconciliation** — `loadProjects` still resolves the
  persisted `selectedProject` against the fresh project list on startup,
  clearing it if the project was removed or refreshing it if stale.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors.

Closes loops.md #13 (TanStack Router).

Files changed: src/{router.tsx (new), App.tsx, store/appStore.ts,
hooks/useProjects.ts, components/{layout/AppShell.tsx, dashboard/Dashboard.tsx,
detail/ProjectDetail.tsx, import/ImportFlow.tsx}},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Standalone Decisions + Loops views

- **Status**: completed
- **Completed**: 2026-07-03

Added two standalone top-level views — Decisions and Loops — that aggregate data
across all projects. Previously this content was only accessible inside
`ProjectDetail`'s tabbed sidebar; now both are first-class views accessible from
the main navigation.

**`DecisionsView.tsx`** (NEW — `src/components/decisions/DecisionsView.tsx`, 240 lines):
- Loads all decisions from every project's `.loopdeck/decisions.md`.
- Filters: free-text search (title, context, consequences, project name),
  status tabs (All / accepted / proposed / superseded), project dropdown.
- Cards grouped by month, each showing project name, date, title, status badge,
  context preview (2-line clamp). Click to expand full context + consequences.
- Colour-coded status: accepted=green, proposed=amber, superseded=muted.
- States: loading spinner, error, empty ("No decisions yet"), filtered-empty
  with "Clear filters" link.

**`LoopsView.tsx`** (NEW — `src/components/loops/LoopsView.tsx`, 210 lines):
- Loads all loop statuses from every project's `.loopdeck/loops.md`.
- Stats bar: active loops, pending next steps, completed history, project count.
- Cards sorted: in-progress loops first, then by project name. Active cards
  have a green-tinted border + background highlight, Play icon, and "Active" tag.
  Completed/paused get CheckCircle/Circle icons.
- Expanded card shows: current loop metadata (started/completed dates), next
  steps list (checked items in green strikethrough, unchecked with arrow icon),
  history summary (last 5 completed loops with "+N more" overflow).
- States: loading, error, empty ("No loops yet").

**Integration:**
- `types/index.ts`: `AppView` gained `"decisions"` and `"loops"`.
- `AppShell.tsx`: Lightbulb and Repeat nav items added.
- `App.tsx`: Both views wired.

**Design decisions:**
- **Project-level sorting for Loops** — active projects first, then
  alphabetical. The user's primary question is "what should I work on next?",
  so active loops must surface above completed ones. Within the same
  activity tier, alphabetical is predictable.
- **Expand-in-place cards vs separate pages** — both views use click-to-
  expand cards instead of navigating to a detail page. Since there's no
  TanStack Router yet, per-card routing would need Zustand state management
  (selected decision ID, selected loop path), which adds complexity without
  clear benefit. The expand-in-place pattern shows detail inline while keeping
  the list visible for context.
- **Month grouping for Decisions** — decisions use month-level buckets
  (not day-level like Activity Feed) because decision dates are date-only
  strings (no time component) and there are typically fewer decisions than
  agent turns. Month grouping avoids single-item day sections.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors.

Closes loops.md #11 (standalone Decisions) and #12 (standalone Loops).

Files changed: src/{types/index.ts, App.tsx,
components/{layout/AppShell.tsx, decisions/DecisionsView.tsx (new),
loops/LoopsView.tsx (new)}}, .loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Activity Feed view (`/activity`)

- **Status**: completed
- **Completed**: 2026-07-03

Added a chronological event feed that aggregates activity from all registered
projects into a single timeline — agent turns, architectural decisions, and
development loop completions, merged and sorted by timestamp.

**`ActivityFeed.tsx`** (NEW — `src/components/activity/ActivityFeed.tsx`, 240 lines):

- **Data collection**: on mount, iterates all registered projects and fetches
  three data sources per project: `agentGetConversation` (turns),
  `getDecisions` (decisions), and `getLoops` (loop history). Each source is
  caught independently — a missing file (e.g. no transcript yet) is skipped
  silently without failing the entire feed.
- **Event unification**: three collectors (`turnsToEvents`, `decisionsToEvents`,
  `loopsToEvents`) normalise each source into a common `ActivityEvent` shape
  with `kind` discriminator (`turn_user` | `turn_assistant` | `turn_error` |
  `decision` | `loop_completed`), project name/path, timestamp, one-line
  summary, and optional detail body.
- **Timeline rendering**: events sorted descending by timestamp, grouped into
  date buckets (Today / Yesterday / "Monday, July 1"), each group shown as a
  section with a sticky date heading, count badge, and a divider line.
- **Event row**: colour-coded icon per kind (User=neutral, Bot=primary,
  Error=destructive, Decision=warning, Loop=success). Project name + timestamp
  on the top row, summary on the second (line-clamped to 2 lines). Click-to-
  expand detail for events with body content (turn text, decision context,
  loop metadata) shown in a monospace block with max-height + scroll.
- **States**: loading spinner, error display, empty state ("No activity yet"
  with Activity icon and guidance text).

**Integration:**
- `types/index.ts`: `AppView` gained `"activity"`.
- `AppShell.tsx`: Activity icon nav item between Dashboard and Agent Runner.
- `App.tsx`: `ActivityFeed` rendered on `currentView === "activity"`.

**Design decisions:**
- **Unified event shape over separate sections** — merging conversations,
  decisions, and loops into a single `ActivityEvent[]` (discriminated by
  `kind`) keeps the timeline rendering logic simple: one sort, one grouping
  pass, one render loop. The alternative (separate sections per source) would
  fragment the chronology and require the user to mentally merge timelines.
- **Best-effort per-source fetching** — each API call per project is tried
  independently; a missing `.loopdeck/sessions/active.jsonl` doesn't prevent
  decisions and loops from appearing. This is important for projects that
  haven't run an agent yet but have decisions/loops from manual edits.
- **Date-only timestamps for decisions/loops** — decisions use `"2026-06-22"`
  format (no time component). We synthesise midnight UTC timestamps
  (`dateToTs`) so they sort correctly within the day's bucket. This means
  decisions always appear at the top of their date group, which is acceptable
  since they're typically batched by session, not by exact hour.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors.

Closes loops.md #10 (Activity Feed view).

Files changed: src/{types/index.ts, App.tsx,
components/{layout/AppShell.tsx, activity/ActivityFeed.tsx (new)}},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Agent Runner view (`/agent`)

- **Status**: completed
- **Completed**: 2026-07-03

Added a standalone, terminal-themed AI agent runner view accessible from the
sidebar. Unlike the project-specific Agent tab in ProjectDetail, the Agent
Runner is a top-level view where the user can switch between projects without
leaving the agent interface.

**`AgentRunner.tsx`** (NEW — `src/components/agent/AgentRunner.tsx`, 370 lines):

- **Left panel** (280px) — scrollable project list showing all registered
  projects with session metadata: live status indicator (green dot for active
  sessions within 30 min), turn count, last activity timestamp, and project
  path. Sorted: live sessions first, then by recency. Empty state when no
  projects are registered.
- **Right panel** — terminal-themed agent interface:
  - Header bar with terminal icon, `loopdeck agent ~/project-name` path, and
    project filesystem path.
  - Toolbar: **Run next loop** (green, starts/continues development loop from
    `.loopdeck/loops.md`) and **Reset** (archive transcript, drop process).
    Turn counter on the right.
  - Scrollable output area with monospace font (`JetBrains Mono`):
    - `❯ user` / `❯ assistant` prompts in green/primary with timestamps and
      token-usage stats
    - Turn content with `whitespace-pre-wrap` preserving agent formatting
    - Empty state: "Select a project" or "No conversation yet" with CTA
  - Live streaming: tool-call activity (diamond `◈` markers in warning color),
    streaming text with green pulsing cursor, busy indicator
  - Composer: green `❯` prompt + single-line input, Enter to send
- **Streaming orchestration**: reuses the same `Channel<ClaudeEvent>` pattern
  as `AgentPanel` — `runStreamingTurn(prompt?)` creates a Channel, wires
  `onmessage` for delta accumulation (`text_delta`, `tool_use`, `result`), and
  calls `agentStartLoopStreaming` / `agentSendMessageStreaming` accordingly.
  Transcript reload on Result event. Mounted guard for channel safety.
- **Session scanning**: on mount, queries all projects for their transcripts
  to derive live/not-live status (active if last assistant turn < 30 min ago).

**Integration:**
- `types/index.ts`: `AppView` gained `"agent"`.
- `AppShell.tsx`: Terminal icon nav item between Dashboard and Import Repo.
- `App.tsx`: `AgentRunner` component rendered on `currentView === "agent"`.

**Design decisions:**
- **Terminal theme not Chat bubble theme** — The Agent Runner uses monospace
  font, dark background (`oklch(0.13_0.01_270)`), prompt indicators (`❯`),
  and flat line-by-line output instead of rounded chat bubbles. This
  distinguishes it from the project-specific Agent Panel and signals
  "developer tool" rather than "chat app".
- **Standalone project selector** — Unlike AgentPanel (reached via Dashboard →
  project → Agent tab), the Agent Runner has its own project list, so the user
  can switch between agent sessions without navigating back to the dashboard.
  This is the "tmux for AI agents" pattern.
- **Shared streaming pattern** — `AgentRunner` does NOT reuse the `Chat`
  component because the terminal aesthetic requires fundamentally different
  rendering (no avatars, no bubbles, prompt-based layout, monospace font). It
  DOES reuse the same streaming orchestration pattern: Channel → `onmessage` →
  delta accumulation → Result → reload. The two components (AgentPanel+Chat,
  AgentRunner) are rendering-siblings but orchestration-cousins — same
  approach, different visual output.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors (3
pre-existing warnings).

Closes loops.md #9 (Agent Runner view).

Files changed: src/{types/index.ts, App.tsx,
components/{layout/AppShell.tsx, agent/AgentRunner.tsx (new)}},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Streaming frontend chat UI with Tauri Channel
- **Status**: completed
- **Completed**: 2026-07-03

Upgraded `AgentPanel.tsx` from batch-only APIs to real-time streaming via Tauri
`Channel<ClaudeEvent>`. The agent's response now renders token-by-token as it
arrives instead of showing a spinner for the full turn duration.

**Backend.**
- `commands.rs`: new `agent_start_loop_streaming` command — builds the next-loop
  prompt server-side (same `build_next_loop_prompt`) and sends via
  `send_and_record_streaming` so the Start-next-loop flow also streams.
- `lib.rs`: registered `agent_start_loop_streaming` in the invoke handler.

**Frontend.**
- `lib/tauri.ts`: `agentStartLoopStreaming(path, onEvent: Channel<ClaudeEvent>)`
  — typed IPC wrapper for the new streaming start-loop command.
- `AgentPanel.tsx`: full rewrite with streaming architecture:
  - `runStreamingTurn(prompt?)` — shared helper that creates a `Channel`,
    wires `onmessage` to accumulate `TextDelta`/`ThinkingDelta`/`Result` events,
    calls the appropriate streaming IPC, and reloads the transcript on completion.
  - Both "Start next loop" and free-form Send use streaming — no batch fallback.
  - `mountedRef` guards channel event handlers against post-unmount state updates.
- `Chat.tsx` (NEW, extracted from AgentPanel) — reusable presentational component:
  - `TurnBubble` — persisted transcript turn (user/assistant/error with usage meta).
  - `ThinkingBlock` — collapsible model reasoning (ChevronDown/ChevronRight toggle).
  - `StreamingBubble` — live token accumulation with typewriter cursor, spinner→Bot
    avatar transition on Result, usage/duration meta in transient window.
  - `Chat` container — scrollable transcript, auto-scroll, empty state, error banner
    with dismiss, composer (Enter to send, Shift+Enter for newline).
  - Pure presentational — no Tauri/Channel imports; all state via props.

**Design decisions:**
- Channel events are the single source of truth for turn state. The invoke
  Promise is only used for infra-level error catching (timeout, no config,
  spawn failure). Model-level errors (`is_error: true`) are surfaced from the
  `Result` event, not from Promise rejection — consistent with the existing
  decision that streaming commands return `()` not `AgentResponse`.
- The `StreamingBubble` renders alongside persisted `TurnBubble`s during a
  turn. When the `Result` event triggers a transcript reload, the streaming
  bubble is naturally replaced by the newly-persisted turn. No imperative
  DOM manipulation — React reconciliation handles the transition.
- Thinking is hidden by default (collapsed) and toggled explicitly. This
  keeps the UI clean during normal turns while making extended thinking
  inspectable when needed.
- **Presentational Chat, streaming orchestration in AgentPanel** — `Chat`
  is a pure rendering component with zero Tauri knowledge, making it reusable
  (e.g. for a standalone `/agent` view) and testable in isolation. `AgentPanel`
  remains the single owner of Channel lifecycle and transcript persistence.
  State flows one way: Channel events → AgentPanel setState → Chat props.

**Verification.** `cargo check` 0 new errors (3 pre-existing warnings);
`cargo test --lib` 108/108 passed (5 ignored live); `tsc --noEmit` clean.

Closes loops.md "Frontend chat UI" step.

Files changed: src-tauri/src/{commands.rs, lib.rs}, src/{lib/tauri.ts,
components/detail/{AgentPanel.tsx, Chat.tsx (new)}}, .loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Streaming `send_message_streaming` with `ClaudeEvent` + `Channel<T>`
- **Status**: completed
- **Completed**: 2026-07-03

Added a streaming variant of `send_message` so the frontend can render assistant
tokens as they arrive instead of waiting for the full turn to buffer. The
streaming path shares the same process/lock/transcript pipeline as the batch
path — only the read loop differs.

**Backend.**
- `agents.rs`: new `ClaudeEvent` enum (Serialize, Clone) with three variants —
  `TextDelta` (one per `ContentBlock::Text`), `ThinkingDelta`, and `Result`
  (terminal; carries the full aggregated `AgentResponse` fields). Made
  `ContentBlock`, `AssistantMessage`, and `RawUsage` `pub(crate)` so
  `claude_session.rs` can iterate content blocks for per-block event emission.
- `claude_session.rs`: `send_message_streaming(&mut self, text, channel:
  &Channel<ClaudeEvent>)` — writes the user turn to stdin (same as `send_message`),
  then reads NDJSON lines, emitting per-content-block `ClaudeEvent`s as each
  `assistant` message arrives, accumulating into `ResponseAccumulator`, and
  emitting the terminal `ClaudeEvent::Result` on turn completion. Channel sends
  are best-effort (`let _ = channel.send(…)`) — a closed channel (frontend
  navigates away) is silently dropped. Same 180s timeout as `send_message`.
- `commands.rs`: `agent_send_message_streaming(path, prompt, on_event:
  Channel<ClaudeEvent>)` Tauri command + `send_and_record_streaming` helper
  (same pre-send user-turn append + post-send assistant-turn append as
  `send_and_record`, but calls `send_message_streaming`). Returns `()`
  rather than `AgentResponse` — the terminal `ClaudeEvent::Result` is the
  single source of truth for the frontend.
- `lib.rs`: registered `agent_send_message_streaming` in the invoke handler.

**Frontend.**
- `types/index.ts`: `ClaudeEvent` discriminated union type mirroring the Rust
  enum (`text_delta`, `thinking_delta`, `result`).
- `lib/tauri.ts`: `agentSendMessageStreaming(path, prompt, onEvent:
  Channel<ClaudeEvent>)` — typed IPC wrapper that passes the Tauri `Channel`
  from `@tauri-apps/api/core`.

**Design decisions:**
- The streaming and batch paths share `ResponseAccumulator` — they can't drift
  in how they aggregate the final response.
- Per-content-block emission (not per-NDJSON-line) matches the UI's natural
  rendering granularity: each `TextDelta` is a complete text fragment.
- `send_message` is *not* refactored to call `send_message_streaming` with a
  dropped channel — the batch path is simpler and doesn't allocate channel
  overhead; the duplication (~15 lines of stdin write) is small.
- Channel dropping is expected and non-fatal: the turn completes regardless,
  and the transcript is always recorded.

**Verification.** `cargo check` 0 new errors (3 pre-existing warnings);
`cargo test --lib` 108/108 passed (4 ignored live); `tsc --noEmit` clean.

Closes loops.md #7 (streaming variant).

Files changed: src-tauri/src/{agents.rs, claude_session.rs, commands.rs,
lib.rs}, src/{types/index.ts, lib/tauri.ts}, .loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Multi-project Claude session orchestration (Start button + Agent tab)
- **Status**: completed
- **Completed**: 2026-07-03

Wired the existing `ClaudeSession` into the LoopDeck UI — pressing **Start** on
a project card now spawns the agent, prompts it for the next loop from
`.loopdeck/loops.md`, and drives work via the orchestrator skill conventions.
Adds true cross-project parallelism, a persisted conversation transcript, and
resume-across-restarts.

**Phase 0 — resume spike (the gate).** The PRD's central risk —
`--resume <id>` composed with `--input-format stream-json` had never been
tested together — was de-risked first with a single ignored integration test
(`test_session_resume_after_restart`): plant a codeword → drop the session
(process dies, simulating restart) → re-spawn with `--resume <id>` → assert
recall. It passed against the live provider, so the full resume path was
built on top of it.

**Backend.**
- `claude_session.rs`: `spawn` gained `resume_session_id: Option<&str>`;
  pushes `--resume <id>` when set.
- `conversation.rs` (NEW): `ConversationTurn` (Serialize/Deserialize) +
  `load_conversation` / `last_session_id` / `append_turn` / `archive_conversation`.
  Storage: `<project>/.loopdeck/sessions/active.jsonl` (append-only JSONL) +
  `archive-<ts>.jsonl` (rotated on reset). 11 offline unit tests.
- `commands.rs`: `AppState.claude_sessions` restructured to the two-layer lock
  (`Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<ClaudeSession>>>>`) — outer
  std Mutex guards the map for microseconds (never across `.await`), inner
  per-project tokio Mutex holds for a full turn so projects run concurrently
  while same-project turns serialize. `with_session` (get-or-spawn, returns
  the Arc), `build_next_loop_prompt` (first unchecked `- [ ]` under
  `## Next Steps`, raw read since `memory::parse_loops` drops the
  checked/unchecked distinction), and `send_and_record` (append user turn
  before send → send → append assistant turn — crash-safe). Four Tauri
  commands: `agent_start_loop`, `agent_send_message`, `agent_get_conversation`,
  `agent_reset_session`.
- `agents.rs`: `UsageInfo` made `pub` + `Deserialize` + `PartialEq` so
  `ConversationTurn` can carry and round-trip it.

**Frontend.**
- `ProjectCard.tsx`: prominent **Start** CTA (Play icon, primary) above the
  icon-tile row.
- `Dashboard.tsx`: wires `onStart` → `setSelectedProject` +
  `setDetailTab("agent")` + `setPendingAgentStart(path)`.
- `ProjectDetail.tsx`: local `useState<DetailTab>` lifted into the store
  (`detailTab` / `setDetailTab`); new `agent` tab (Bot icon) renders
  `<AgentPanel>`.
- `AgentPanel.tsx` (NEW): loads transcript on mount; auto-fires
  `agent_start_loop` when `pendingAgentStart` matches; renders chat bubbles
  (user/assistant/error styling, usage + duration meta); Start / Send
  (free-form, Enter-to-send) / New conversation controls; spinner while busy.
- `types/index.ts`, `lib/tauri.ts`, `store/appStore.ts`: `"agent"` tab,
  `ConversationTurn` / `AgentResponse` / `UsageInfo` types, four typed IPC
  wrappers, `detailTab` + `pendingAgentStart` store state.

**Verification.** `cargo check` 0 errors; `cargo clippy` 0 new warnings (8
pre-existing untouched); offline `cargo test --lib` 108 passed; live
`cargo test --lib claude_session -- --ignored` 4/4 passed (existing 3 +
resume); `tsc --noEmit` clean. Manual `tauri dev` UI verification left to the
user.

Closes loops.md #4 (per-project turn lock), #5 (`with_session`), #6
(`agent_send_message`).

Files changed: src-tauri/src/{claude_session.rs, conversation.rs (new),
commands.rs, agents.rs, lib.rs}, src/{types/index.ts, lib/tauri.ts,
store/appStore.ts, components/dashboard/{ProjectCard.tsx, Dashboard.tsx},
components/detail/{ProjectDetail.tsx, AgentPanel.tsx (new)}},
.loopdeck/loops.md.

### 2026-07-03 — ClaudeSession resilience hardening (checkpoint 4)
- **Status**: completed
- **Completed**: 2026-07-03

Closed the three foundation gaps flagged in checkpoint 3, plus a real Drop
bug found via clippy along the way.

- **stderr-drain task**: `Stdio::null()` → `Stdio::piped()` + a background
  `tokio::spawn` that loops `read_line` on stderr and logs each non-empty line
  at debug (`[claude stderr] …`), exiting on EOF. Eliminates the latent
  deadlock where a verbose child fills its OS pipe buffer with nowhere to go.
  Handle stored as `stderr_drain: Option<JoinHandle<()>>`; `Drop` aborts it
  defensively after reaping the child.
- **`send_message` timeout**: wrapped the whole turn (stdin write + read-until-
  `result`) in `tokio::time::timeout` with a new `SEND_MESSAGE_TIMEOUT`
  const (180s). A stuck peer now fails loudly as `AppError::Agent("…
  timed out after 180s")` instead of hanging the caller. Post-timeout the
  session is left mid-turn — documented as "drop, don't resend".
- **Drop restructure (bonus bug fix)**: clippy's `let_underscore_future`
  warning surfaced a real defect — the force-kill path used `let _ =
  self.child.kill()` / `wait()`, but tokio's `kill()`/`wait()` return
  *futures*; dropping them unawaited meant the SIGKILL was never sent and the
  child was leaked, not killed. Rewrote `Drop` into two clear phases (graceful
  5s EOF window → forceful `start_kill()` + 2s reap) sharing a new
  `poll_reap(child, window)` helper (non-blocking `try_wait()` paced with
  `thread::sleep`). `start_kill()` is synchronous — the correct tokio API for
  a sync `Drop`.

Verified: `cargo check` + `cargo clippy` clean for new code (remaining
warnings are pre-existing dead-code that clears once `ClaudeSession` is wired
into a Tauri command). All 3 live integration tests pass (`test_session_single_turn`,
`test_session_current_directory`, `test_session_retains_context_across_turns`) —
the stream-json protocol is unaffected and `Drop` still reaps cleanly.

Next up: #4 per-project turn lock, #5 `with_session` helper, #6 `agent_send_message`
Tauri command.

Files changed: src-tauri/src/claude_session.rs.

### 2026-07-02 — ClaudeSession async/tokio migration (checkpoint 3)
- **Status**: completed
- **Completed**: 2026-07-02

Migrated `ClaudeSession` from `std::process` to `tokio::process` to prepare for
concurrent multi-project sessions and streaming. `send_message` is now `async`,
reading stdout via `AsyncBufReadExt::read_line().await` and writing stdin via
`AsyncWriteExt::write_all().await`. `Drop` rewritten for tokio: `start_kill()`
plus a bounded `try_wait()` reap loop (no `.await` in `Drop`).

Two bugs fixed along the way: (1) missing `\n` on NDJSON writes — lost when
`writeln!` became `write_all` during the migration, it silently broke the input
protocol and stalled reads; surfaced by removing a stray `.ok()` that was
swallowing the write error. (2) `Stdio::piped()` stderr with no reader — latent
deadlock risk, switched to `Stdio::null()` for now (proper drain task is a
next step).

Also introduced a `CommandEnv` trait so `apply_agent_config` is generic over
both std and tokio `Command` — keeps the offline env-var inspection tests
working via std's `get_envs()` while production uses tokio. `spawn` now takes
`project_path` and calls `cmd.current_dir(project_path)` so claude runs in the
project, not in loopdeck's cwd.

Live integration tests pass against the provider (single-turn, current-directory,
and cross-turn context retention). Commits `4a10df9` (config foundation) and
`192ac8a` (ClaudeSession) on `feat/claude-session`.

Files changed: src-tauri/src/{claude_session.rs, agents.rs, commands.rs, lib.rs,
config.rs}, src/{types/index.ts, lib/tauri.ts, App.tsx, components/{layout/AppShell.tsx, settings/Settings.tsx}}.

### 2026-06-22 — V2 Agent Memory Layer
- **Status**: completed
- **Completed**: 2026-06-22

Full V2 agent memory layer implemented across 4 phases:

**Phase 1 — Backend (rust-expert):** memory.rs with lenient Markdown parser for decisions.md
(architectural decision records) and loops.md (current loop, next steps, history). Two new IPC
commands: get_decisions, get_loops. 22 unit tests covering edge cases (em dash, hyphen,
empty files, missing headings, partial file creation).

**Phase 2 — Frontend (vite-senior-engineer):** DecisionsPanel and LoopsPanel components
with loading/empty/error states. Sidebar tab navigation in ProjectDetail (Overview |
Decisions | Loops). All matches existing Zustand + typed IPC conventions.

**Phase 3 — Agent Convention:** Project-local .claude/skills/orchestrator SKILL.md extending
the global orchestrator with .loopdeck/ write conventions. CLAUDE.md updated with memory
convention. settings.local.json Stop hook with dual approach: command hook with
hookSpecificOutput.additionalContext (injects memory reminder into model context) and shell
script (mechanical heartbeat fallback). Initial implementation used `type: "prompt"` which
silently failed — fixed by switching to `type: "command"` with JSON output. Hook verified
working via pipe-test and jq validation.

**Phase 4 — Review:** rust-code-reviewer and vite-senior-engineer review completed. One
medium finding (leading-newline split pattern) fixed. 5 additional edge case tests added.
Final: 52 Rust tests passing, TypeScript clean, 12 IPC commands registered.

Files created: memory.rs (610 lines), DecisionsPanel.tsx, LoopsPanel.tsx (both ~120 lines),
DecisionsPanel.css, LoopsPanel.css, orchestrator SKILL.md, loopdeck-memory-write.sh.
Updated: ProjectDetail.tsx/CSS (sidebar nav), types/index.ts, tauri.ts, lib.rs, commands.rs,
CLAUDE.md, settings.local.json, .loopdeck/decisions.md (6 decisions), .loopdeck/loops.md.

### 2026-06-22 — V2 Agent Memory Backend
- **Status**: completed
- **Completed**: 2026-06-22

Created memory.rs with Markdown parser for decisions.md and loops.md. Two new IPC commands:
get_decisions and get_loops. 18 new tests. All 47 tests passing.

### 2026-06-22 — V1 Gaps
- **Status**: completed
- **Completed**: 2026-06-22

Fixed 4 V1 gaps: scan_depth enforcement, last_opened on dashboard, detected_stack +
description_preview on import, rescan_project command. 30→30 tests (added max_depth test).

### 2026-06-22 — V1 Core
- **Status**: completed
- **Completed**: 2026-06-22

Built the scanner, .loopdeck/ bootstrap, project config registry, and full React UI.
10 IPC commands: scan, import, list, get, update_description, remove, open_in_finder,
open_in_terminal, regenerate_description, rescan_project. 30 Rust tests passing.
