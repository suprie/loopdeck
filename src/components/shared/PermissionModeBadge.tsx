import { ShieldCheck } from "lucide-react";

/**
 * Persistent indicator of LoopDeck's effective agent permission mode, shown in
 * agent headers so the user knows what gates what before they start a turn.
 *
 * Today only `ConfirmChanges` is wired (the default + only mode). When Phase 3
 * adds `AutonomousProject` as a real per-project opt-in, this component becomes
 * mode-aware — until then it's a single honest label.
 *
 * See docs/PRD-trust-boundary-hardening.md FR1 and the loops.md Gate A "Honest
 * permission default" item.
 */
export function PermissionModeBadge() {
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
