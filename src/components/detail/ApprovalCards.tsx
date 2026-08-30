import { useState } from "react";
import {
  CheckCircle2,
  ShieldCheck,
  ShieldPlus,
  ShieldX,
  XCircle,
} from "lucide-react";
import type { ApprovalDecision, PlanApprovalDecision } from "../../types";
import { Markdown } from "../shared/Markdown";
import { describeTool, sanitise } from "./chatUtils";

export function PermissionApprovalCard({
  toolName,
  input,
  disabled,
  onDecide,
  onAlwaysAllow,
}: {
  toolName: string;
  input: string;
  disabled?: boolean;
  onDecide: (decision: ApprovalDecision) => void;
  onAlwaysAllow?: () => void;
}) {
  const [reason, setReason] = useState("");
  const summary = describeTool(toolName, input);
  return (
    <div className="my-2 rounded-lg border border-primary/30 bg-[color-mix(in_oklab,var(--primary)_5%,transparent)] p-3 space-y-3">
      <div className="flex items-center gap-2 text-xs font-medium text-primary">
        <span className="inline-block size-1.5 rounded-full bg-primary animate-pulse" />
        <span>The agent needs your approval</span>
      </div>
      <div className="rounded-md border border-border bg-input/60 px-3 py-2">
        <div className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground mb-0.5">
          {toolName}
        </div>
        <div className="font-mono text-xs text-foreground/90 break-all leading-relaxed">
          {sanitise(summary)}
        </div>
      </div>
      <details className="text-xs text-muted-foreground">
        <summary className="cursor-pointer hover:text-foreground transition-colors select-none">
          Add a reason (optional, deny only)
        </summary>
        <textarea
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          disabled={disabled}
          rows={2}
          placeholder="Why deny? (shown to the agent)"
          className="mt-2 w-full resize-none rounded-md border border-border bg-input px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
        />
      </details>
      <div className="flex justify-end gap-2 pt-1">
        <button
          type="button"
          disabled={disabled}
          onClick={() =>
            onDecide({ allow: false, reason: reason.trim() || undefined })
          }
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-[color-mix(in_oklab,var(--destructive)_40%,transparent)] text-destructive text-xs font-medium hover:bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <ShieldX className="size-3.5" />
          Deny
        </button>
        <button
          type="button"
          disabled={disabled}
          onClick={() => onDecide({ allow: true })}
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-primary/40 text-primary text-xs font-medium hover:bg-primary/10 transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <ShieldCheck className="size-3.5" />
          Allow once
        </button>
        {onAlwaysAllow && (
          <button
            type="button"
            disabled={disabled}
            onClick={onAlwaysAllow}
            title="Allow now and remember this rule for future sessions"
            className="inline-flex items-center gap-1.5 h-8 px-4 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <ShieldPlus className="size-3.5" />
            Always allow
          </button>
        )}
      </div>
    </div>
  );
}

export function PlanApprovalCard({
  plan,
  disabled,
  onDecide,
}: {
  plan: string;
  disabled?: boolean;
  onDecide: (decision: PlanApprovalDecision) => void;
}) {
  const [feedback, setFeedback] = useState("");
  return (
    <div className="my-2 rounded-lg border border-primary/30 bg-[color-mix(in_oklab,var(--primary)_5%,transparent)] p-3 space-y-3">
      <div className="flex items-center gap-2 text-xs font-medium text-primary">
        <span className="inline-block size-1.5 rounded-full bg-primary animate-pulse" />
        <span>The agent has a plan ready for review</span>
      </div>
      <div className="rounded-md border border-border bg-input/60 px-3 py-2 max-h-64 overflow-y-auto text-sm">
        <Markdown>{plan}</Markdown>
      </div>
      <details className="text-xs text-muted-foreground">
        <summary className="cursor-pointer hover:text-foreground transition-colors select-none">
          Add feedback (optional, reject only)
        </summary>
        <textarea
          value={feedback}
          onChange={(e) => setFeedback(e.target.value)}
          disabled={disabled}
          rows={2}
          placeholder="What should the agent change? (shown to the agent)"
          className="mt-2 w-full resize-none rounded-md border border-border bg-input px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
        />
      </details>
      <div className="flex justify-end gap-2 pt-1">
        <button
          type="button"
          disabled={disabled}
          onClick={() =>
            onDecide({ approve: false, feedback: feedback.trim() || undefined })
          }
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-[color-mix(in_oklab,var(--destructive)_40%,transparent)] text-destructive text-xs font-medium hover:bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <XCircle className="size-3.5" />
          Reject
        </button>
        <button
          type="button"
          disabled={disabled}
          onClick={() => onDecide({ approve: true })}
          className="inline-flex items-center gap-1.5 h-8 px-4 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <CheckCircle2 className="size-3.5" />
          Approve &amp; execute
        </button>
      </div>
    </div>
  );
}
