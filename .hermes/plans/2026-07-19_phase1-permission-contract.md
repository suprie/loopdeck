# Phase 1 — Permission Contract Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Make LoopDeck's effective agent permission policy explicit, testable, and consistent across the Claude spawn flags, generated `.claude/settings.json`, LoopDeck's own policy, and the approval UI — with `ConfirmChanges` as the honest default.

**Architecture:** The four-arm `answer_control_request` flow in `claude_session.rs` (AskUserQuestion → destructive floor → `MANUAL_APPROVAL_TOOLS` interception → synchronous auto-policy) is already confirm-first by construction. This phase fixes the three layers that silently bypass it: (1) the `acceptEdits` spawn flag auto-approves file edits before LoopDeck sees them, (2) the curated `settings.json` allowlist auto-approves broad `Edit(*)`/`Write(*)`/build-runners at the Claude layer, and (3) the `allow_by_default()` naming contradicts the actual behavior. No flow change — just honesty.

**Tech Stack:** Rust (backend), React + TypeScript (frontend), Tauri IPC, vitest (not yet wired — backend tests only for this phase).

**Source of truth:** `docs/PRD-trust-boundary-hardening.md` FR1 + Phase 1; `.loopdeck/loops.md` Gate A item 1.

---

## Current Contradictions (reference — do not change in this plan)

1. **`src-tauri/src/claude_session.rs:218-228`** — code runs `--permission-mode acceptEdits`, doc comment directly above says "Run in `default` permission mode". `acceptEdits` auto-approves Edit/Write/NotebookEdit inside Claude, so the `MANUAL_APPROVAL_TOOLS` entries for those three are dead code today.
2. **`src-tauri/src/skills.rs:300-328`** — curated allowlist includes `Edit(*)`, `Write(*)`, `Bash(cargo:*)`, `Bash(npm:*)`, `Bash(npx:*)`, `Bash(go:*)`, `Bash(pnpm:*)`, `Bash(yarn:*)`. These match at the Claude layer before any `control_request` reaches LoopDeck.
3. **`src-tauri/src/permission.rs:63-117`** — `PolicyDefault::Allow` + `PermissionPolicy::allow_by_default()` names imply auto-approve-everything; actual behavior is confirm-first because `MANUAL_APPROVAL_TOOLS` interception runs first in `answer_control_request`.
4. **`src/components/detail/Chat.tsx:849`** — `PermissionApprovalCard` has no effective-mode indicator.

---

## Pre-flight (Task 0): baseline verification

**Objective:** Confirm the tree is green before changing anything.

**Step 1:** Run baseline
```
cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test --lib
cd .. && npm run build
```
Expected: all pass (257 tests / 0 failed / 8 ignored as of last commit). If not, stop and fix before proceeding.

---

## Task 1: Flip `acceptEdits` → `default` (highest-leverage change)

**Objective:** Route every un-ruled tool call through LoopDeck's policy instead of letting Claude auto-approve file edits.

**Files:**
- Modify: `src-tauri/src/claude_session.rs:218-228`

**Step 1:** Read the current block to confirm exact contents:
```
sed -n '216,230p' src-tauri/src/claude_session.rs
```

**Step 2:** Apply the one-line change. Replace:
```rust
        // Run in `default` permission mode so EVERY tool call that doesn't
        // match an allow rule emits a `control_request` — that's the whole
        // point of this PRD: LoopDeck gets to observe and decide each one.
        // (Previously this used `acceptEdits`, which only auto-approves file
        // edits and left every Bash call stalling — see
        // docs/PRD-agent-permission-stall.md for the evidence.)
        cmd.args(["--permission-mode", "acceptEdits"]);
```
with:
```rust
        // `default` permission mode: every tool call that doesn't match an
        // allow rule in `.claude/settings.json` emits a `control_request`
        // that LoopDeck decides (floor → manual approval → auto-policy).
        // This is the honest single-source-of-truth: nothing is silently
        // auto-approved by Claude itself. Earlier iterations used
        // `acceptEdits`, which auto-approves Edit/Write/NotebookEdit inside
        // Claude and made the `MANUAL_APPROVAL_TOOLS` entries for those tools
        // dead code. See docs/PRD-trust-boundary-hardening.md FR1.
        cmd.args(["--permission-mode", "default"]);
```

**Step 3:** Verify it compiles + tests still pass:
```
cd src-tauri && cargo check --lib && cargo test --lib
```
Expected: 257 passed / 0 failed (no behavior change observable to offline tests — they don't spawn claude).

**Step 4:** Commit
```
git add src-tauri/src/claude_session.rs
git commit -m "fix(permission): use --permission-mode default so Claude routes all un-ruled calls to LoopDeck"
```

---

## Task 2: Remove broad auto-allow rules from generated `settings.json`

**Objective:** Stop silently auto-approving `Edit(*)`, `Write(*)`, and broad build-runners at the Claude layer.

**Files:**
- Modify: `src-tauri/src/skills.rs:300-328` (the `DEFAULT_ALLOW` array — confirm the exact name)
- Modify: `src-tauri/src/skills.rs` test `test_setup_hooks_writes_curated_allowlist` (~line 723)

**Step 1:** Read the current allowlist:
```
sed -n '284,350p' src-tauri/src/skills.rs
```

**Step 2:** Remove these eight entries from the array literal:
- `"Edit(*)",`
- `"Write(*)",`
- `"Bash(cargo:*)",`
- `"Bash(npm:*)",`
- `"Bash(npx:*)",`
- `"Bash(go:*)",`
- `"Bash(pnpm:*)",`
- `"Bash(yarn:*)",`

Keep everything else (the read-only `Bash(ls:*)`, `Bash(cat:*)`, `Bash(git status:*)`, etc.). Update the comment above the array to explain why broad mutation/runner rules are excluded — a hostile repo controls its own scripts and build steps.

**Step 3:** Update the test. Find `test_setup_hooks_writes_curated_allowlist` and change its assertions: remove (or invert) the assertions that `has("Bash(cargo:*)")` and `has("Bash(npm:*)")`, and add assertions that the removed rules are *absent*:
```rust
        assert!(!has("Edit(*)"), "broad Edit rule must NOT be seeded");
        assert!(!has("Write(*)"), "broad Write rule must NOT be seeded");
        assert!(!has("Bash(cargo:*)"), "broad build-runner rule must NOT be seeded");
        assert!(!has("Bash(npm:*)"), "broad build-runner rule must NOT be seeded");
```

**Step 4:** Run the test to verify it passes with the new assertions:
```
cd src-tauri && cargo test --lib skills::tests::test_setup_hooks_writes_curated_allowlist
```
Expected: PASS.

**Step 5:** Run the full suite:
```
cargo test --lib
```
Expected: 257 passed (test count unchanged — only assertions changed).

**Step 6:** Commit
```
git add src-tauri/src/skills.rs
git commit -m "fix(permission): remove broad Edit(*)/Write(*)/build-runner rules from generated settings.json"
```

---

## Task 3: Rename `PolicyDefault`/`allow_by_default` to honest names

**Objective:** Make the type names match the actual confirm-first behavior.

**Files:**
- Modify: `src-tauri/src/permission.rs:60-117`
- Modify: `src-tauri/src/commands.rs:1870,2475`

**Step 1:** In `permission.rs`, rename the enum and constructor:

Replace (lines ~60-91):
```rust
pub enum PolicyDefault {
    Allow,
    #[allow(dead_code)]
    Deny,
}

#[derive(Debug, Clone, Copy)]
pub struct PermissionPolicy {
    default: PolicyDefault,
}

impl PermissionPolicy {
    pub fn allow_by_default() -> Self {
        Self {
            default: PolicyDefault::Allow,
        }
    }

    #[allow(dead_code)]
    pub fn with_default(default: PolicyDefault) -> Self {
        Self { default }
    }
    ...
}
```
with:
```rust
/// The effective permission mode for un-ruled, floor-clearing tool calls.
///
/// Mirrors the two user-facing modes from the PRD. `ConfirmChanges` is the
/// default; `AutonomousProject` is the per-project opt-in (not yet wired to
/// a config surface — deferred per Gate A).
pub enum PermissionMode {
    /// Default. Read-only tools auto-allow; mutating/executing tools park on
    /// a manual-approval card via `MANUAL_APPROVAL_TOOLS`. This is what the
    /// v1 "allow_by_default" posture actually was — the manual-approval
    /// interception in `answer_control_request` runs before this default.
    ConfirmChanges,
    /// Per-project opt-in: file mutation inside the canonical project root
    /// may proceed automatically. Command execution, MCP, and operations
    /// outside the project still follow their policy. NOT YET WIRED — kept
    /// as a future-proofing variant matching the PRD's two-mode contract.
    #[allow(dead_code)]
    AutonomousProject,
}

#[derive(Debug, Clone, Copy)]
pub struct PermissionPolicy {
    mode: PermissionMode,
}

impl PermissionPolicy {
    /// The locked default: mutating/executing tools require manual approval,
    /// read-only tools auto-allow, the destructive floor always applies.
    /// Constructed once per session in `commands.rs`.
    pub fn confirm_changes() -> Self {
        Self {
            mode: PermissionMode::ConfirmChanges,
        }
    }

    /// Constructor for tests / a future config surface.
    #[allow(dead_code)]
    pub fn with_mode(mode: PermissionMode) -> Self {
        Self { mode }
    }
    ...
}
```

**Step 2:** Update the `decide` method's match arms:
```rust
        match self.mode {
            PermissionMode::ConfirmChanges => Decision::Allow,
            PermissionMode::AutonomousProject => Decision::Allow,
        }
```
Wait — both arms allow today (the confirm-first behavior comes from `MANUAL_APPROVAL_TOOLS` interception in `claude_session.rs`, not from `decide`). Add a doc comment explaining this so future readers don't think `decide` is where the confirm logic lives:
```rust
    /// Decide whether a tool request should be allowed or denied.
    ///
    /// **Scope:** this is the *fallback* decision for requests that clear the
    /// destructive floor AND are not intercepted by `MANUAL_APPROVAL_TOOLS`
    /// (which parks on a UI approval card before this method is consulted —
    /// see `claude_session.rs::answer_control_request`). In practice that
    /// means this only governs read-only tools (Read/Grep/Glob/WebSearch)
    /// and unknown tool names. The mode distinction becomes meaningful once
    /// `AutonomousProject` is wired; under `ConfirmChanges` both arms allow
    /// because the gating already happened upstream.
    pub fn decide(&self, tool_name: &str, input: &Value) -> Decision {
        if let Some(reason) = check_destructive_floor(tool_name, input) {
            return Decision::Deny(reason);
        }
        match self.mode {
            PermissionMode::ConfirmChanges => Decision::Allow,
            PermissionMode::AutonomousProject => Decision::Allow,
        }
    }
```

**Step 3:** Update the module-level doc comment (lines 8-17) to reflect the honest posture:
```rust
//! ## Posture
//!
//! **Confirm-changes by default.** Read-only tools (Read/Grep/Glob/WebSearch)
//! auto-allow; mutating and executing tools (Bash/Edit/Write/NotebookEdit/
//! WebFetch/MCP) park on a manual-approval card until the user decides. A
//! destructive-command floor (rm -rf, force-push, pipe-to-shell, …) is always
//! enforced as a hard deny, regardless of mode, so a mis-clicked "Allow" can't
//! trash the user's system. The mode governs only what happens to requests
//! that clear the floor AND aren't intercepted by the manual-approval set —
//! in `ConfirmChanges` that's read-only tools only.
```

**Step 4:** Update the two construction sites in `commands.rs`:
```
grep -n "allow_by_default" src-tauri/src/commands.rs
```
Replace `PermissionPolicy::allow_by_default()` with `PermissionPolicy::confirm_changes()` at both sites (~lines 1870 and 2475).

**Step 5:** Update the tests in `permission.rs`. Search for `allow_by_default` and `with_default`:
```
grep -n "allow_by_default\|with_default\|PolicyDefault" src-tauri/src/permission.rs
```
- `allow_by_default_lets_unknown_commands_through` → rename to `confirm_changes_lets_read_only_tools_through`, swap `PermissionPolicy::allow_by_default()` → `PermissionPolicy::confirm_changes()`
- `allow_by_default_still_enforces_destructive_floor` → rename to `confirm_changes_still_enforces_destructive_floor`
- `deny_by_default_denies_unknown_commands` and `deny_by_default_still_enforces_floor` — these construct `PolicyDefault::Deny`, which no longer exists. Either delete them (the deny-by-default posture isn't part of the PRD's two-mode contract) or convert them to construct `PermissionMode::AutonomousProject` and assert the unchanged allow behavior (since both arms allow today). Prefer deletion — they test a posture that's being removed.

**Step 6:** Verify:
```
cd src-tauri && cargo fmt && cargo check --lib && cargo clippy --all-targets && cargo test --lib
```
Expected: compiles, 0 new clippy warnings, test count drops by however many deny-by-default tests you deleted (likely -2 → 255 passed).

**Step 7:** Commit
```
git add src-tauri/src/permission.rs src-tauri/src/commands.rs
git commit -m "refactor(permission): rename allow_by_default → confirm_changes to match actual behavior"
```

---

## Task 4: Add a permission-path regression test suite

**Objective:** Pin the full decision matrix under `ConfirmChanges` so future changes can't silently regress which tools gate how.

**Files:**
- Modify: `src-tauri/src/permission.rs` `#[cfg(test)] mod tests`

**Step 1:** Add a new test that documents the end-to-end decision routing. Note: `permission.rs` is pure logic and can't test the `MANUAL_APPROVAL_TOOLS` interception (that lives in `claude_session.rs` and requires a live channel/session). The regression contract for the full matrix is therefore split:

- `permission.rs` tests: floor + fallback-decide behavior
- `requires_manual_approval()` tests: which tool names gate (already in `mutating_tools_require_approval` / `read_only_tools_skip_approval` / `mcp_tools_require_approval_regardless_of_capability`)

Add a single consolidated test that documents the *intended* matrix as a table, so a reader can see the whole contract in one place:
```rust
    #[test]
    fn confirm_changes_decision_matrix_documented() {
        // Documents the Phase 1 decision matrix under ConfirmChanges.
        // Floor denies are tested exhaustively elsewhere; this pins the
        // fallback behavior + the manual-approval set as a single contract.
        let policy = PermissionPolicy::confirm_changes();

        // Read-only tools: auto-allow (clear the floor, not in
        // MANUAL_APPROVAL_TOOLS).
        for tool in ["Read", "Grep", "Glob", "WebSearch"] {
            assert_eq!(policy.decide(tool, &json!({})), Decision::Allow,
                "{tool} should auto-allow under ConfirmChanges");
        }

        // Mutating tools clear the floor with non-Bash inputs (the floor only
        // inspects Bash commands), so `decide` returns Allow — BUT
        // `requires_manual_approval` returns true, so `answer_control_request`
        // parks them on a UI card before `decide`'s result is used. This test
        // pins both layers so a future change to either is visible.
        for tool in ["Edit", "Write", "NotebookEdit", "WebFetch"] {
            assert_eq!(policy.decide(tool, &json!({})), Decision::Allow,
                "{tool} clears the floor (non-Bash)");
            assert!(requires_manual_approval(tool),
                "{tool} must be in MANUAL_APPROVAL_TOOLS");
        }

        // Bash with a safe command: clears floor, BUT still in
        // MANUAL_APPROVAL_TOOLS → parks on UI card.
        assert_eq!(
            policy.decide("Bash", &json!({ "command": "ls -la" })),
            Decision::Allow
        );
        assert!(requires_manual_approval("Bash"));

        // MCP tools: always manual-approval regardless of capability.
        assert!(requires_manual_approval("mcp__github__create_pull_request"));
        assert!(requires_manual_approval("mcp__filesystem__read_file"));

        // Destructive floor: hard deny regardless of mode.
        for cmd in ["rm -rf /", "git push --force", "sudo rm x"] {
            assert!(matches!(
                policy.decide("Bash", &json!({ "command": cmd })),
                Decision::Deny(_)
            ), "{cmd} must be denied by the floor");
        }
    }
```

**Step 2:** Run:
```
cargo test --lib permission::tests::confirm_changes_decision_matrix_documented
```
Expected: PASS.

**Step 3:** Full suite:
```
cargo test --lib
```
Expected: previous count + 1.

**Step 4:** Commit
```
git add src-tauri/src/permission.rs
git commit -m "test(permission): pin full ConfirmChanges decision matrix as a single contract"
```

---

## Task 5: Surface the effective mode in the Agent UI

**Objective:** Make the confirm-changes posture visible so users understand what's gated vs. silent. Scope: a small badge in the agent header, no settings toggle (AutonomousProject is deferred).

**Files:**
- Modify: `src/components/detail/AgentPanel.tsx` (header area — find the existing toolbar/header)
- Modify: `src/components/agent/AgentRunner.tsx` (terminal header — find the existing header bar)
- Optional: `src/components/detail/Chat.tsx` if the approval card should show "Confirm changes mode" context

**Step 1:** Pick a shared component location. If there's a shared `AgentHeader` or similar, use it; otherwise add a small inline badge to both headers. Look:
```
grep -n "header\|Header\|toolbar\|Toolbar" src/components/detail/AgentPanel.tsx | head
grep -n "header\|Header\|toolbar\|Toolbar" src/components/agent/AgentRunner.tsx | head
```

**Step 2:** Design the badge. Small, muted, with a shield icon (lucide-react `ShieldCheck` is already imported in `Chat.tsx`). Text: "Confirm changes". Tooltip: "File edits, commands, and network calls require your approval. Read-only tools run automatically."

Example JSX:
```tsx
<span
  className="inline-flex items-center gap-1 text-[10px] font-medium text-muted-foreground px-1.5 py-0.5 rounded border border-border"
  title="File edits, commands, and network calls require your approval. Read-only tools run automatically."
>
  <ShieldCheck className="size-3" />
  Confirm changes
</span>
```

**Step 3:** Verify the build:
```
npm run build
```
Expected: passes (tsc + vite).

**Step 4:** Manual smoke check (optional but recommended): `npm run tauri dev`, open the Agent panel + Agent Runner, confirm the badge renders.

**Step 5:** Commit
```
git add src/components/detail/AgentPanel.tsx src/components/agent/AgentRunner.tsx
git commit -m "feat(ui): surface Confirm changes mode badge in agent headers"
```

---

## Task 6: Update loops.md + decisions.md

**Objective:** Close the loop with a history entry and a decision record.

**Files:**
- Modify: `.loopdeck/loops.md` (add History entry at top of `## History`)
- Modify: `.loopdeck/decisions.md` (append new decision)

**Step 1:** Add to top of `## History` in loops.md — follow the existing entry format (Status / Completed / narrative / Changes / Design decisions / Verification / Files changed). Date: 2026-07-19. Title: "Phase 1 — Honest permission contract (ConfirmChanges default)".

**Step 2:** Add to decisions.md — new section `## 2026-07-19 — ConfirmChanges as the default permission mode`. Cover: the three layers that contradicted each other (acceptEdits flag, broad settings.json allowlist, misleading `allow_by_default` name), the decision to make confirm-first honest rather than add a new mode, the deferral of `AutonomousProject` config surface (per Gate A), and the decision to keep read-only Bash rules in the curated allowlist while removing broad mutation/runner rules.

**Step 3:** Commit
```
git add .loopdeck/loops.md .loopdeck/decisions.md
git commit -m "docs(permission): record Phase 1 ConfirmChanges contract"
```

---

## Task 7: Final verification + Gate A item check-off

**Objective:** Confirm the whole batch is green and mark the Gate A item done.

**Step 1:** Full verify:
```
cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test --lib
cd .. && npm run build
```
Expected: all pass.

**Step 2:** In `.loopdeck/loops.md` under "Release Gate A", change the first item from `- [ ]` to `- [x]`:
```
- [x] **Honest permission default:** ship `ConfirmChanges` first; remove generated `Edit(*)`, `Write(*)`, and broad build-runner rules; align Claude spawn settings, LoopDeck policy, approval UI, and regression tests
```

**Step 3:** Commit
```
git add .loopdeck/loops.md
git commit -m "chore(loops): check off Gate A — honest permission default"
```

---

## Risks / Tradeoffs / Open Questions

- **More approval prompts.** Flipping `acceptEdits` → `default` means file edits the agent used to do silently now park on an approval card. This is the intended behavior (PRD FR1), but it's a UX change. Mitigation: the "Always allow" button on the approval card persists a narrow rule to `settings.local.json`, so prompt fatigue is one click per tool/command pattern. The PRD explicitly accepts this tradeoff for the alpha.
- **`AutonomousProject` is a type variant with no config surface.** Some reviewers may flag this as YAGNI violation. Rationale: the PRD's two-mode contract is explicit, and having the variant in the type makes the intent clear without exposing a config path that isn't ready. Marked `#[allow(dead_code)]` so clippy doesn't complain. If you prefer, drop it entirely and re-add when AutonomousProject lands — the rename to `PermissionMode::ConfirmChanges` (singular) is still an improvement over `allow_by_default`.
- **Curated allowlist reduction may break the agent's ability to run tests/builds without prompting.** That's the point — a hostile repo controls its `package.json` scripts. The user can re-add `Bash(npm run test:*)` etc. via "Always allow" once they trust the project, which is exactly the PRD's intended narrow-rule flow.
- **Live integration tests (8 ignored) are not re-run.** They exercise the spawn path against a real `claude` + provider and need `LOOPDECK_TEST_AUTH_TOKEN`. The spawn flag change is observable there but the offline tests assert the policy layer, which is unaffected. Recommend running the ignored suite manually before tagging the alpha.

## Out of Scope (deferred per Gate A / PRD)

- `AutonomousProject` config surface + UI toggle
- Permission mode in `ProjectMeta` / per-project persistence
- Bounded expiry for parked approvals/questions (separate Gate A item)
- Approval-card a11y improvements (P6)
- Cross-boundary smoke test (Gate B)
