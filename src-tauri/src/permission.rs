//! Tool-permission policy for the LoopDeck-spawned Claude agent.
//!
//! When Claude emits a `control_request` (because a tool call didn't match a
//! `.claude/settings.json` allow rule), this module decides whether LoopDeck
//! answers `allow` or `deny`. The decision is the single source of truth that
//! `ClaudeSession` writes back as a `control_response`.
//!
//! ## Posture (v1)
//!
//! **Allow-by-default with a destructive floor.** The agent is non-interactive
//! (no TTY) and meant to drive itself through complete loops, so a mode that
//! can prompt is a latent stall (see `docs/PRD-agent-permission-stall.md`).
//! Routing every un-ruled request through here — rather than
//! `--dangerously-skip-permissions` — means each decision is *observable*
//! (logged + surfaced to the UI) and the posture is one explicit switch. A
//! short deny-list of obviously destructive patterns is still enforced so the
//! floor survives even under allow-by-default.
//!
//! This module is pure logic (no I/O) so it's unit-testable without spawning
//! `claude` or hitting a provider.

use serde_json::Value;

// ── Decision ───────────────────────────────────────────────────────────────

/// The outcome of a permission check.
///
/// `Deny` carries a human-readable reason surfaced to the model (as the
/// `control_response` message) and to the UI/logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
}

impl Decision {
    /// `"allow"` or `"deny"` — the wire value for `control_response.behavior`.
    pub fn behavior(&self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny(_) => "deny",
        }
    }

    /// The reason string, empty for a plain allow. Used in logs/UI narration.
    pub fn reason(&self) -> &str {
        match self {
            Decision::Allow => "",
            Decision::Deny(r) => r,
        }
    }

    /// True when this is a `Deny`.
    #[allow(dead_code)]
    pub fn is_deny(&self) -> bool {
        matches!(self, Decision::Deny(_))
    }
}

// ── Policy ─────────────────────────────────────────────────────────────────

/// The fallback posture for tool requests that match no deny rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDefault {
    /// Allow any un-ruled request (the v1 posture). Most permissive; never
    /// stalls on the control protocol.
    Allow,
    /// Deny any un-ruled request. Safest, but the agent effectively can't run
    /// commands autonomously since `acceptEdits` doesn't cover Bash.
    ///
    /// Not used by the v1 production path (which is allow-by-default) but
    /// constructed in unit tests and the deny-path integration test.
    #[allow(dead_code)]
    Deny,
}

/// The configurable permission policy. The deny floor is always in effect;
/// `default` only governs requests that fall through it.
#[derive(Debug, Clone, Copy)]
pub struct PermissionPolicy {
    default: PolicyDefault,
}

impl PermissionPolicy {
    /// The locked v1 posture: allow un-ruled requests, keep the destructive
    /// floor. Constructed once per session in `commands.rs`.
    pub fn allow_by_default() -> Self {
        Self {
            default: PolicyDefault::Allow,
        }
    }

    /// Constructor for tests / a future config surface.
    #[allow(dead_code)]
    pub fn with_default(default: PolicyDefault) -> Self {
        Self { default }
    }

    /// Decide whether a tool request should be allowed or denied.
    ///
    /// The floor (destructive Bash patterns) is checked first under either
    /// posture; only requests that clear it consult `default`.
    pub fn decide(&self, tool_name: &str, input: &Value) -> Decision {
        // The floor applies regardless of `default`: even allow-by-default
        // blocks commands that are destructive by their very shape.
        if let Some(reason) = check_destructive_floor(tool_name, input) {
            return Decision::Deny(reason);
        }

        match self.default {
            PolicyDefault::Allow => Decision::Allow,
            PolicyDefault::Deny => Decision::Deny(String::from(
                "no matching allow rule and LoopDeck is deny-by-default",
            )),
        }
    }
}

// ── Manual approval set ────────────────────────────────────────────────────

/// Tools that mutate the filesystem or execute commands / hit the network and
/// therefore require explicit human approval before the agent may run them.
///
/// The agent runs autonomously and edits/commits the user's project, so calls
/// that change state (vs. read-only `Read`/`Grep`/`Glob`/`WebSearch`) are
/// gated behind a manual Allow/Deny card: the agent turn parks until the user
/// decides. This is the v1 manual-approval surface; anything not listed here
/// falls through to the (silent, allow-by-default) policy.
///
/// Case-sensitive match — Claude emits tool names in PascalCase exactly as
/// written here. `AskUserQuestion` is deliberately NOT in this list: it has
/// its own dedicated parking path (it carries structured questions, not a
/// yes/no approval), and listing it here would double-prompt.
const MANUAL_APPROVAL_TOOLS: &[&str] = &["Bash", "Edit", "Write", "NotebookEdit", "WebFetch"];

/// Whether a tool call needs explicit human approval before it may run.
///
/// Consulted by `ClaudeSession::answer_control_request` *after* the destructive
/// floor but *before* the auto-allow policy: floor-matching calls are denied
/// outright, manual-approval calls park the turn on the UI, everything else
/// is silently allowed. Keeping this as a function (not a public set the
/// caller iterates) lets the policy stay the single source of "what gates
/// how".
///
/// **MCP tools** (`mcp__<server>__<tool>`) are always gated. Their capabilities
/// are unknown to LoopDeck — a server can expose anything from a read-only
/// lookup to a mutating GitHub `create_pull_request` — so the safe default is
/// to ask. This also matches Claude Code's own posture, which prompts on every
/// MCP call regardless of permission mode. Read-only MCP tools can be added to
/// the project's `.claude/settings.json` allow list to short-circuit the
/// prompt, same as built-in tools.
pub fn requires_manual_approval(tool_name: &str) -> bool {
    MANUAL_APPROVAL_TOOLS.contains(&tool_name) || is_mcp_tool(tool_name)
}

/// Whether a tool name refers to an MCP server tool.
///
/// MCP tool names follow `mcp__<server>__<tool>` (double-underscore separated,
/// server + tool segments). We match the prefix rather than counting segments
/// so a future naming variant still gates — the prefix is the stable signal.
fn is_mcp_tool(tool_name: &str) -> bool {
    tool_name.starts_with("mcp__")
}

// ── Destructive floor ──────────────────────────────────────────────────────

/// A command pattern to refuse, matched as a prefix on the normalized command.
struct DenyRule {
    tool: &'static str,
    /// Lowercased prefix matched against the leading portion of the command.
    /// Prefix (not exact) so e.g. `rm -rf /` and `rm -rf ~/x` both trip the
    /// `rm -rf` rule without enumerating every path.
    prefix: &'static str,
    reason: &'static str,
}

/// The destructive floor. Hard-coded rather than config-driven: these are
/// commands that are destructive by shape, not by policy preference, so they
/// shouldn't silently become allowed if someone flips the default posture.
const DESTRUCTIVE_FLOOR: &[DenyRule] = &[
    DenyRule {
        tool: "Bash",
        prefix: "rm -rf",
        reason: "recursive forced delete blocked by policy floor",
    },
    DenyRule {
        tool: "Bash",
        prefix: "sudo",
        reason: "privileged execution blocked by policy floor",
    },
    DenyRule {
        tool: "Bash",
        prefix: "git push --force",
        reason: "force push blocked by policy floor",
    },
    DenyRule {
        tool: "Bash",
        prefix: "git push -f",
        reason: "force push blocked by policy floor",
    },
    DenyRule {
        tool: "Bash",
        prefix: ":(){", // fork bomb
        reason: "known dangerous pattern blocked by policy floor",
    },
];

/// Check the request against the destructive floor. Returns `Some(reason)` when
/// the request should be denied, `None` when it clears the floor.
///
/// Only `Bash` commands carry a `command` string we can pattern-match on; other
/// tools (Read/Edit/Write/…) always clear the floor — their risk is bounded by
/// the project's git history, which is recoverable.
fn check_destructive_floor(tool_name: &str, input: &Value) -> Option<String> {
    // No command to inspect → clears the floor (can't pattern-match a non-Bash
    // or malformed input against the deny rules).
    let command = input.get("command").and_then(Value::as_str)?;

    // Normalize: trim leading whitespace so `  rm -rf` still trips the rule.
    let trimmed = command.trim_start();
    let lower = trimmed.to_ascii_lowercase();

    for rule in DESTRUCTIVE_FLOOR {
        if tool_name == rule.tool && lower.starts_with(rule.prefix) {
            return Some(String::from(rule.reason));
        }
    }
    None
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash(command: &str) -> (&'static str, Value) {
        ("Bash", json!({ "command": command }))
    }

    // ── Decision helpers ────────────────────────────────────────────────

    #[test]
    fn decision_behavior_and_reason() {
        assert_eq!(Decision::Allow.behavior(), "allow");
        assert_eq!(Decision::Allow.reason(), "");
        assert!(!Decision::Allow.is_deny());

        let deny = Decision::Deny(String::from("nope"));
        assert_eq!(deny.behavior(), "deny");
        assert_eq!(deny.reason(), "nope");
        assert!(deny.is_deny());
    }

    // ── requires_manual_approval ───────────────────────────────────────

    #[test]
    fn mutating_tools_require_approval() {
        for tool in ["Bash", "Edit", "Write", "NotebookEdit", "WebFetch"] {
            assert!(
                requires_manual_approval(tool),
                "{tool} should require manual approval"
            );
        }
    }

    #[test]
    fn read_only_tools_skip_approval() {
        // Read-only / non-mutating tools flow straight through the policy
        // (silent allow-by-default) — no prompt. AskUserQuestion is excluded
        // too: it has its own dedicated parking path.
        for tool in [
            "Read",
            "Grep",
            "Glob",
            "WebSearch",
            "Task",
            "TodoWrite",
            "AskUserQuestion",
            "McpCustom",
            "UnknownTool",
        ] {
            assert!(
                !requires_manual_approval(tool),
                "{tool} should NOT require manual approval"
            );
        }
    }

    #[test]
    fn manual_approval_match_is_case_sensitive() {
        // Claude emits PascalCase tool names; a lowercase variant must NOT
        // match — otherwise a mis-cased tool call would silently skip the
        // prompt. If Claude ever changes casing, that's a behaviour change we
        // want to notice, not paper over.
        assert!(requires_manual_approval("Bash"));
        assert!(!requires_manual_approval("bash"));
        assert!(!requires_manual_approval("BASH"));
    }

    // ── MCP tools are always gated ─────────────────────────────────────

    #[test]
    fn mcp_tools_require_approval_regardless_of_capability() {
        // MCP tool capabilities are opaque to LoopDeck — a server can expose
        // anything from a read-only lookup to a mutating create_pull_request.
        // The safe default is to gate every mcp__* call; read-only ones can be
        // allow-listed in .claude/settings.json to short-circuit the prompt.
        assert!(requires_manual_approval("mcp__github__create_pull_request"));
        assert!(requires_manual_approval("mcp__filesystem__read_file"));
        assert!(requires_manual_approval("mcp__my_server__my_tool"));
    }

    #[test]
    fn mcp_prefix_match_is_case_sensitive() {
        // The `mcp__` prefix is emitted lowercase by Claude; a PascalCase
        // lookalike must NOT trip the gate (consistent with built-in tools).
        assert!(!requires_manual_approval("MCP__github__x"));
        assert!(!requires_manual_approval("Mcp__github__x"));
    }

    #[test]
    fn non_mcp_double_underscore_names_are_not_gated_by_prefix() {
        // Guard against an over-eager prefix: a tool name that happens to start
        // with `mcp` but isn't the wire prefix must not be gated as MCP.
        // Only the exact lowercase `mcp__` prefix counts.
        assert!(!requires_manual_approval("mcprunner__x"));
        assert!(!requires_manual_approval("mcp_other"));
    }

    // ── allow-by-default posture (the v1 default) ──────────────────────

    #[test]
    fn allow_by_default_lets_unknown_commands_through() {
        let policy = PermissionPolicy::allow_by_default();
        assert_eq!(policy.decide("Bash", &json!({ "command": "cargo test" })), Decision::Allow);
        assert_eq!(policy.decide("Bash", &json!({ "command": "git add ." })), Decision::Allow);
        assert_eq!(policy.decide("Edit", &json!({ "file_path": "/a.rs" })), Decision::Allow);
        // A tool/input we've never seen → still allowed (the whole point).
        assert_eq!(policy.decide("McpCustom", &json!({})), Decision::Allow);
    }

    #[test]
    fn allow_by_default_still_enforces_destructive_floor() {
        let policy = PermissionPolicy::allow_by_default();
        for cmd in ["rm -rf /", "rm -rf ~/stuff", "  rm -rf target/", "sudo rm x"] {
            let (tool, input) = bash(cmd);
            let dec = policy.decide(tool, &input);
            assert!(matches!(dec, Decision::Deny(_)), "{cmd} should be denied");
        }
        // Force push variants.
        assert!(matches!(
            policy.decide("Bash", &json!({ "command": "git push --force origin main" })),
            Decision::Deny(_)
        ));
        assert!(matches!(
            policy.decide("Bash", &json!({ "command": "git push -f" })),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn destructive_denials_carry_a_reason() {
        let policy = PermissionPolicy::allow_by_default();
        let dec = policy.decide("Bash", &json!({ "command": "rm -rf target" }));
        match dec {
            Decision::Deny(r) => assert!(r.contains("policy floor"), "reason: {r}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    // ── deny-by-default posture (exercised for completeness) ───────────

    #[test]
    fn deny_by_default_denies_unknown_commands() {
        let policy = PermissionPolicy::with_default(PolicyDefault::Deny);
        assert!(matches!(
            policy.decide("Bash", &json!({ "command": "cargo test" })),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn deny_by_default_still_enforces_floor() {
        // Floor is checked before the default, so a destructive command is
        // denied for the floor reason (not the generic default reason).
        let policy = PermissionPolicy::with_default(PolicyDefault::Deny);
        match policy.decide("Bash", &json!({ "command": "rm -rf /" })) {
            Decision::Deny(r) => assert!(r.contains("policy floor"), "reason: {r}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    // ── floor edge cases ───────────────────────────────────────────────

    #[test]
    fn floor_is_case_insensitive_and_trims_leading_whitespace() {
        let policy = PermissionPolicy::allow_by_default();
        // Uppercase + leading spaces must still trip.
        assert!(matches!(
            policy.decide("Bash", &json!({ "command": "   RM -RF target" })),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn floor_ignores_non_bash_tools() {
        // An Edit with an `rm`-ish path doesn't carry a command and is bounded
        // by git history → clears the floor under either posture.
        let policy = PermissionPolicy::allow_by_default();
        assert_eq!(
            policy.decide("Edit", &json!({ "file_path": "/etc/passwd" })),
            Decision::Allow
        );
    }

    #[test]
    fn floor_passes_through_when_no_command_field() {
        // A Bash input without a command string (malformed/edge) can't be
        // inspected → clears the floor rather than guessing.
        let policy = PermissionPolicy::allow_by_default();
        assert_eq!(policy.decide("Bash", &json!({})), Decision::Allow);
    }

    #[test]
    fn safe_commands_that_look_similar_are_allowed() {
        // Guard against an over-eager prefix: `git push` (no force) and
        // `rm file.txt` (single file, no -rf) must stay allowed.
        let policy = PermissionPolicy::allow_by_default();
        assert_eq!(
            policy.decide("Bash", &json!({ "command": "git push origin main" })),
            Decision::Allow
        );
        assert_eq!(
            policy.decide("Bash", &json!({ "command": "rm scratch.txt" })),
            Decision::Allow
        );
        // `rmdir` starts with `rm` but not `rm -rf` → allowed.
        assert_eq!(
            policy.decide("Bash", &json!({ "command": "rmdir empty/" })),
            Decision::Allow
        );
    }
}
