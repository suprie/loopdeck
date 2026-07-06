import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { Save, Check, Eye, EyeOff, ChevronDown } from "lucide-react";

import { PageHeader } from "@/components/app-shell";

export const Route = createFileRoute("/settings")({
  component: SettingsPage,
});

function SettingsPage() {
  const [showKey, setShowKey] = useState(false);
  const [saved, setSaved] = useState(false);

  return (
    <>
      <PageHeader title="Settings" subtitle="Configure your AI agent provider" />

      <div className="mx-auto w-full max-w-2xl px-8 py-8">
        <div className="mb-4 flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          <span className="size-1.5 rounded-full bg-primary" />
          Agent Configuration
        </div>

        <div className="rounded-xl border border-border bg-card p-6 shadow-[var(--shadow-sm)]">
          <Field label="Auth Token" hint="Your API key. Stored locally in ~/.config/loopdeck/config.">
            <div className="relative">
              <input
                type={showKey ? "text" : "password"}
                defaultValue="sk-ant-01234567890abcdef"
                className="h-9 w-full rounded-md border border-input bg-background px-3 pr-9 text-sm font-mono outline-none focus:border-primary/50 focus:ring-2 focus:ring-ring/20"
              />
              <button
                type="button"
                onClick={() => setShowKey((s) => !s)}
                className="absolute right-2 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded text-muted-foreground hover:text-foreground"
              >
                {showKey ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
              </button>
            </div>
          </Field>

          <Field label="Base URL" hint="Provider endpoint. Leave blank for default.">
            <input
              type="text"
              defaultValue="https://api.anthropic.com"
              className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none focus:border-primary/50 focus:ring-2 focus:ring-ring/20"
            />
          </Field>

          <div className="grid grid-cols-2 gap-4">
            <Field label="Model">
              <Select value="claude-sonnet-4-6" />
            </Field>
            <Field label="Effort Level">
              <Select value="High" />
            </Field>
          </div>
        </div>

        <div className="mt-6 flex items-center gap-3">
          <button
            type="button"
            onClick={() => {
              setSaved(true);
              setTimeout(() => setSaved(false), 2500);
            }}
            className="inline-flex h-9 items-center gap-1.5 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
          >
            <Save className="size-4" />
            Save Configuration
          </button>
          {saved && (
            <span className="inline-flex items-center gap-1.5 text-xs text-success">
              <Check className="size-3.5" />
              Configuration saved successfully
            </span>
          )}
        </div>
      </div>
    </>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-5 last:mb-0">
      <label className="mb-1.5 block text-xs font-medium">{label}</label>
      {children}
      {hint && <p className="mt-1.5 text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  );
}

function Select({ value }: { value: string }) {
  return (
    <button
      type="button"
      className="flex h-9 w-full items-center justify-between rounded-md border border-input bg-background px-3 text-sm outline-none transition-colors hover:bg-accent focus:border-primary/50 focus:ring-2 focus:ring-ring/20"
    >
      <span>{value}</span>
      <ChevronDown className="size-3.5 text-muted-foreground" />
    </button>
  );
}
