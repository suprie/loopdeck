import { useState, useEffect, useCallback } from "react";
import { Save, Loader2, Check, Settings2, Eye, EyeOff, FileText, FolderOpen } from "lucide-react";
import { PageHeader } from "../layout/AppShell";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { useAppStore } from "../../store/appStore";
import {
  getAgentConfig,
  setAgentConfig,
  clearAuthToken,
  getLogInfo,
  revealLogDir,
} from "../../lib/tauri";
import type { AgentConfig, LogInfo } from "../../types";

const EFFORT_OPTIONS = [
  { value: "low", label: "Low — fastest, least thorough" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "max", label: "Max — slowest, most thorough" },
] as const;

const CLAUDE_MODEL_PRESETS = [
  "claude-sonnet-4-6",
  "claude-opus-4-8",
  "claude-haiku-4-5",
  "deepseek-v4-pro[1m]",
] as const;

const INITIAL_FORM: AgentConfig = {
  harness: "claude",
  auth_token: "",
  base_url: "",
  model: "",
  effort: "high",
};

export function Settings() {
  const setError = useAppStore((s) => s.setError);

  const [form, setForm] = useState<AgentConfig>(INITIAL_FORM);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [hasToken, setHasToken] = useState(false);
  const [clearing, setClearing] = useState(false);

  // Load current config on mount
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const existing = await getAgentConfig();
        if (!cancelled && existing) {
          setForm({
            harness: existing.harness ?? "claude",
            // Never pre-fill the plaintext token — it isn't returned over IPC.
            // The field stays empty; `hasToken` drives the "stored" affordance.
            auth_token: "",
            base_url: existing.base_url ?? "",
            model: existing.model ?? "",
            effort: existing.effort ?? "high",
          });
          setHasToken(!!existing.has_auth_token);
        }
      } catch {
        // Config doesn't exist yet — that's fine, use defaults
      } finally {
        if (!cancelled) setLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleChange = useCallback(
    (field: keyof AgentConfig, value: string) => {
      setForm((prev) => ({ ...prev, [field]: value }));
      setSaved(false);
    },
    [],
  );

  const handleSave = useCallback(async () => {
    setSaving(true);
    setSaved(false);
    try {
      // Strip empty strings to None on the Rust side. An empty auth_token is
      // omitted entirely so the backend preserves the existing stored token
      // rather than clearing it.
      const toSave: AgentConfig = {};
      toSave.harness = form.harness ?? "claude";
      if (form.auth_token) toSave.auth_token = form.auth_token;
      if (form.base_url) toSave.base_url = form.base_url;
      if (form.model) toSave.model = form.model;
      if (form.effort) toSave.effort = form.effort;

      const saved = await setAgentConfig(toSave);
      setHasToken(!!saved.has_auth_token);
      // The token now lives in the local secrets file — clear the field so the
      // masked "stored" affordance shows and the plaintext doesn't linger in
      // state.
      setForm((prev) => ({ ...prev, auth_token: "" }));
      setSaved(true);
      setTimeout(() => setSaved(false), 2500);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }, [form, setError]);

  const handleClearToken = useCallback(async () => {
    setClearing(true);
    try {
      await clearAuthToken();
      setHasToken(false);
      setForm((prev) => ({ ...prev, auth_token: "" }));
      setSaved(true);
      setTimeout(() => setSaved(false), 2500);
    } catch (err) {
      setError(String(err));
    } finally {
      setClearing(false);
    }
  }, [setError]);

  if (!loaded) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="size-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col min-h-0">
      <PageHeader title="Settings" subtitle="Configure your AI agent provider" />

      <div className="flex-1 min-h-0 overflow-y-auto px-8 py-8">
        <div className="mx-auto w-full max-w-2xl">
          {/* Section heading */}
          <div className="mb-4 flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Settings2 className="size-3.5" />
            Agent Configuration
          </div>

          <div className="rounded-xl border border-border bg-card p-6 shadow-[var(--shadow-sm)]">
            <Field
              label="Agent Harness"
              hint="Choose the local CLI LoopDeck uses for conversations, tools, approvals, and resume."
            >
              <Select
                value={form.harness ?? "claude"}
                onValueChange={(v) => {
                  setForm((previous) => ({
                    ...previous,
                    harness: v as "claude" | "codex",
                    model: "",
                  }));
                  setSaved(false);
                }}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="claude">Claude Code</SelectItem>
                  <SelectItem value="codex">Codex</SelectItem>
                </SelectContent>
              </Select>
            </Field>

            {form.harness === "codex" ? (
              <div className="mb-5 rounded-md border border-border bg-muted/30 px-3 py-2.5 text-xs text-muted-foreground">
                Codex uses the account and provider settings from your installed Codex CLI.
              </div>
            ) : (
              <>
                {/* Auth Token */}
                <Field
                  label="Auth Token"
                  hint="Your API key, stored locally in an owner-only file — never written to the registry. Leave blank to keep the stored token; type a new value to replace it."
                >
                  <div className="relative">
                    <Input
                      type={showKey ? "text" : "password"}
                      value={form.auth_token ?? ""}
                      onChange={(e) => handleChange("auth_token", e.target.value)}
                      placeholder={
                        hasToken ? "•••••••• (stored — type to replace)" : "sk-abc123..."
                      }
                      className="pr-9 font-mono"
                    />
                    <button
                      type="button"
                      onClick={() => setShowKey((s) => !s)}
                      title={showKey ? "Hide token" : "Show token"}
                      className="absolute right-2 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded text-muted-foreground transition-colors hover:text-foreground"
                    >
                      {showKey ? (
                        <EyeOff className="size-3.5" />
                      ) : (
                        <Eye className="size-3.5" />
                      )}
                    </button>
                  </div>
                  {hasToken && !form.auth_token ? (
                    <div className="mt-2 flex items-center justify-between rounded-md border border-success/30 bg-success/5 px-2.5 py-1.5">
                      <span className="inline-flex items-center gap-1.5 text-[11px] text-success">
                        <Check className="size-3.5" /> Token stored locally
                      </span>
                      <button
                        type="button"
                        onClick={handleClearToken}
                        disabled={clearing}
                        className="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-destructive disabled:opacity-50"
                      >
                        {clearing ? <Loader2 className="size-3 animate-spin" /> : null}
                        Clear stored token
                      </button>
                    </div>
                  ) : null}
                </Field>

                {/* Base URL */}
                <Field label="Base URL" hint="Provider endpoint. Leave blank for default Anthropic API.">
                  <Input
                    type="text"
                    value={form.base_url ?? ""}
                    onChange={(e) => handleChange("base_url", e.target.value)}
                    placeholder="https://api.anthropic.com"
                  />
                </Field>
              </>
            )}

            {/* Model + Effort row */}
            <div className="grid grid-cols-2 gap-4">
              <Field
                label="Model"
                hint={
                  form.harness === "codex"
                    ? "Leave blank to use the Codex CLI default, or enter a model ID."
                    : "Model ID. Pick a preset or type your own."
                }
              >
                {form.harness !== "codex" ? (
                  <Select
                    value={form.model ?? ""}
                    onValueChange={(v) => handleChange("model", v === "__custom__" ? "" : v)}
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder="Select a model…" />
                    </SelectTrigger>
                    <SelectContent>
                      {CLAUDE_MODEL_PRESETS.map((m) => (
                        <SelectItem key={m} value={m}>
                          {m}
                        </SelectItem>
                      ))}
                      <SelectItem value="__custom__">Custom (type below)</SelectItem>
                    </SelectContent>
                  </Select>
                ) : null}
                {form.harness === "codex" ||
                (form.model ?? "") === "" ||
                !CLAUDE_MODEL_PRESETS.some((model) => model === form.model) ? (
                  <Input
                    type="text"
                    value={form.model ?? ""}
                    onChange={(e) => handleChange("model", e.target.value)}
                    placeholder={
                      form.harness === "codex" ? "Use Codex default" : "claude-sonnet-4-6"
                    }
                    className={form.harness === "codex" ? "" : "mt-2"}
                  />
                ) : null}
              </Field>

              <Field label="Effort Level" hint="Higher effort = more thorough reasoning, slower responses.">
                <Select
                  value={form.effort ?? "high"}
                  onValueChange={(v) => handleChange("effort", v)}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {EFFORT_OPTIONS.map((opt) => (
                      <SelectItem key={opt.value} value={opt.value}>
                        {opt.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
            </div>
          </div>

          {/* Save */}
          <div className="mt-6 flex items-center gap-3">
            <button
              onClick={handleSave}
              disabled={saving}
              className="inline-flex items-center gap-1.5 h-9 px-4 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50"
            >
              {saving ? (
                <>
                  <Loader2 className="size-4 animate-spin" />
                  Saving…
                </>
              ) : (
                <>
                  <Save className="size-4" />
                  Save Configuration
                </>
              )}
            </button>
            {saved && (
              <span className="inline-flex items-center gap-1.5 text-xs text-success">
                <Check className="size-3.5" />
                Configuration saved successfully
              </span>
            )}
          </div>

          {/* Diagnostics — where the logs live + bounded-retention summary.
              Surfaces the user-accessible log folder and the retention cap so
              "logs grow forever" is a visible, bounded quantity, not magic. */}
          <Diagnostics />
        </div>
      </div>
    </div>
  );
}

// ── Diagnostics ──────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  return `${mb.toFixed(1)} MB`;
}

function Diagnostics() {
  const setError = useAppStore((s) => s.setError);
  const [info, setInfo] = useState<LogInfo | null>(null);
  const [opening, setOpening] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setInfo(await getLogInfo());
    } catch (err) {
      setError(String(err));
    }
  }, [setError]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const handleOpen = useCallback(async () => {
    setOpening(true);
    try {
      await revealLogDir();
      // Re-read after opening so the file list reflects any external changes
      // (e.g. the user deletes a file from the now-open folder).
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setOpening(false);
    }
  }, [refresh, setError]);

  const dir = info?.dir ?? null;
  const fileCount = info?.files.length ?? 0;

  return (
    <>
      <div className="mt-10 mb-4 flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        <FileText className="size-3.5" />
        Diagnostics
      </div>

      <div className="rounded-xl border border-border bg-card p-6 shadow-[var(--shadow-sm)]">
        {!info ? (
          <div className="flex items-center justify-center py-2">
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          </div>
        ) : dir ? (
          <div className="space-y-4">
            <div>
              <Label className="mb-1.5 block text-xs font-medium">Log folder</Label>
              <p
                className="truncate rounded-md bg-muted/40 px-2.5 py-1.5 font-mono text-[11px] text-muted-foreground"
                title={dir}
              >
                {dir}
              </p>
              <p className="mt-1.5 text-[11px] leading-relaxed text-muted-foreground">
                {fileCount} {fileCount === 1 ? "file" : "files"} · {formatBytes(info.total_bytes)} ·
                keeping the last {info.max_files} daily logs.
              </p>
            </div>

            <button
              onClick={handleOpen}
              disabled={opening}
              className="inline-flex items-center gap-1.5 h-9 px-3.5 rounded-md border border-border bg-background text-sm font-medium text-foreground transition-colors hover:bg-muted/50 disabled:opacity-50"
            >
              {opening ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <FolderOpen className="size-4" />
              )}
              Open logs folder
            </button>
          </div>
        ) : (
          <p className="text-[11px] leading-relaxed text-muted-foreground">
            Logging is stderr-only — no log folder is available on disk. This is
            unexpected outside of a misconfigured/headless environment.
          </p>
        )}
      </div>
    </>
  );
}

// ── Field primitive ──────────────────────────────────────────────────────────

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
      <Label className="mb-1.5 block text-xs font-medium">{label}</Label>
      {children}
      {hint && <p className="mt-1.5 text-[11px] leading-relaxed text-muted-foreground">{hint}</p>}
    </div>
  );
}
