import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { Bot, User, ChevronRight, Plus, Send, Shield } from "lucide-react";

import { PageHeader } from "@/components/app-shell";
import { sampleChat, type ChatMessage } from "@/lib/mock-data";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/agent")({
  component: AgentPage,
});

function AgentPage() {
  const [messages] = useState<ChatMessage[]>(sampleChat);
  const [draft, setDraft] = useState("");

  return (
    <>
      <PageHeader
        title="Agent · loopdeck"
        subtitle="Refactor chat UI · streaming"
        actions={
          <button
            type="button"
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-transparent px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <Plus className="size-3.5" />
            New chat
          </button>
        }
      />

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex-1 space-y-6 overflow-y-auto px-8 py-6">
          {messages.map((m) => (
            <MessageRow key={m.id} m={m} />
          ))}
        </div>

        <div className="border-t border-border bg-surface px-8 py-4">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              setDraft("");
            }}
            className="mx-auto flex max-w-3xl items-end gap-2"
          >
            <div className="flex-1 rounded-lg border border-border bg-input focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-ring/20">
              <textarea
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                placeholder="Send a follow-up message…"
                rows={2}
                className="block w-full resize-none rounded-lg bg-transparent px-3 py-2 text-sm outline-none placeholder:text-muted-foreground"
              />
            </div>
            <button
              type="submit"
              className="flex size-9 items-center justify-center rounded-md bg-primary text-primary-foreground transition-opacity hover:opacity-90"
            >
              <Send className="size-4" />
            </button>
          </form>
          <p className="mx-auto mt-2 max-w-3xl text-[10px] text-muted-foreground">
            Enter to send · Shift+Enter for newline
          </p>
        </div>
      </div>
    </>
  );
}

function MessageRow({ m }: { m: ChatMessage }) {
  if (m.role === "user") {
    return (
      <div className="flex justify-end gap-3">
        <div className="max-w-[75%] rounded-lg rounded-br-sm bg-primary px-3.5 py-2 text-sm text-primary-foreground">
          {m.content}
        </div>
        <Avatar icon={User} />
      </div>
    );
  }

  if (m.approval) {
    return (
      <div className="flex gap-3">
        <Avatar icon={Bot} muted />
        <div className="max-w-[75%] flex-1 rounded-lg border border-primary/30 bg-primary/5 p-4">
          <div className="flex items-center gap-2 text-sm font-medium">
            <span className="size-1.5 rounded-full bg-primary" />
            {m.approval.title}
          </div>
          <div className="mt-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {m.approval.label}
          </div>
          <div className="mt-1 font-mono text-xs">{m.approval.detail}</div>
          <div className="mt-4 flex justify-end gap-2">
            <button
              type="button"
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-transparent px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              <Shield className="size-3.5" />
              Deny
            </button>
            <button
              type="button"
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
            >
              <Shield className="size-3.5" />
              Allow
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex gap-3">
      <Avatar icon={Bot} muted />
      <div className="flex max-w-[75%] flex-1 flex-col gap-2">
        {m.thinking && (
          <details className="text-[11px] text-muted-foreground">
            <summary className="cursor-pointer list-none">
              <ChevronRight className="mr-1 inline size-3 transition-transform group-open:rotate-90" />
              Show thinking ({m.thinking.length.toLocaleString()} chars)
            </summary>
            <p className="mt-2 rounded-md border border-border bg-muted/40 p-2 leading-relaxed">
              {m.thinking}
            </p>
          </details>
        )}
        {m.toolCalls && (
          <div className="space-y-1">
            {m.toolCalls.map((t, i) => (
              <div
                key={i}
                className="flex items-baseline gap-2 font-mono text-[11px] text-muted-foreground"
              >
                <span className="text-primary">›</span>
                <span className="font-medium">{t.kind}</span>
                <span>·</span>
                <span>{t.detail}</span>
              </div>
            ))}
          </div>
        )}
        {m.content && (
          <div className="rounded-lg border border-border bg-card px-4 py-3 text-sm leading-relaxed">
            {m.content}
            <span className="ml-0.5 inline-block h-4 w-[1.5px] translate-y-0.5 animate-pulse bg-primary" />
          </div>
        )}
        {m.meta && (
          <div className="text-[10px] text-muted-foreground">
            {m.meta.timeSec}s · {m.meta.tokensIn.toLocaleString()}→
            {m.meta.tokensOut.toLocaleString()} tok · ${m.meta.costUsd.toFixed(4)}
          </div>
        )}
      </div>
    </div>
  );
}

function Avatar({
  icon: Icon,
  muted,
}: {
  icon: React.ComponentType<{ className?: string }>;
  muted?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex size-8 shrink-0 items-center justify-center rounded-full",
        muted
          ? "border border-border bg-surface text-muted-foreground"
          : "bg-primary text-primary-foreground",
      )}
    >
      <Icon className="size-4" />
    </div>
  );
}
