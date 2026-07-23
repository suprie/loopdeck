import { ShieldCheck, ShieldAlert } from "lucide-react";

/**
 * Persistent indicator of LoopDeck's effective agent permission mode, shown in
 * agent headers so the user knows what gates what before they start a turn.
 *
 * Mode-aware: `confirm` (the default) renders a green "Confirm changes" label;
 * `autonomous` renders an amber "Autonomous" label. Under autonomous mode the
 * agent self-approves floor-clearing tool calls (Edit/Write, safe Bash, MCP,
 * WebFetch) so loops run unattended — the destructive floor (`rm -rf`,
 * force-push, `curl|sh`, `sudo`, …) still applies regardless.
 *
 * Defaults to `confirm` when no prop is passed (back-compat for render sites
 * that haven't been wired to the per-project flag yet).
 *
 * See docs/PRD-trust-boundary-hardening.md FR1 and the loops.md Gate A "Honest
 * permission default" item.
 */
export function PermissionModeBadge({
  mode = "confirm",
}: {
  mode?: "confirm" | "autonomous";
}) {
  if (mode === "autonomous") {
    return (
      <span
        className="inline-flex items-center gap-1 text-[10px] font-medium text-amber-600 dark:text-amber-400 px-1.5 py-0.5 rounded border border-amber-500/40 bg-amber-500/10"
        title="Autonomous mode: the agent self-approves tool calls (edits, commands, MCP) so loops run unattended. The destructive floor still applies (rm -rf, force-push, curl|sh, sudo are denied). Review the resulting pull requests before merging."
      >
        <ShieldAlert className="size-3" />
        Autonomous
      </span>
    );
  }
  return (
    <span
      className="inline-flex items-center gap-1 text-[10px] font-medium text-muted-foreground px-1.5 py-0.5 rounded border border-border"
      title="File edits, commands, and network calls require your approval. Read-only tools run automatically. Add narrow always-allow rules from an approval card when you trust a command."
    >
      <ShieldCheck className="size-3" />
      Confirm changes
    </span>
  );
}
