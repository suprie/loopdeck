import { useState, useEffect, useCallback } from "react";
import { Save, Loader2, Check, Settings2, Eye, EyeOff } from "lucide-react";
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
import { getAgentConfig, setAgentConfig } from "../../lib/tauri";
import type { AgentConfig } from "../../types";

const EFFORT_OPTIONS = [
  { value: "low", label: "Low — fastest, least thorough" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "max", label: "Max — slowest, most thorough" },
] as const;

const MODEL_PRESETS = [
  "claude-sonnet-4-6",
  "claude-opus-4-8",
  "claude-haiku-4-5",
  "deepseek-v4-pro[1m]",
] as const;

const INITIAL_FORM: AgentConfig = {
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

  // Load current config on mount
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const existing = await getAgentConfig();
        if (!cancelled && existing) {
          setForm({
            auth_token: existing.auth_token ?? "",
            base_url: existing.base_url ?? "",
            model: existing.model ?? "",
            effort: existing.effort ?? "high",
          });
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
      // Strip empty strings to None on the Rust side
      const toSave: AgentConfig = {};
      if (form.auth_token) toSave.auth_token = form.auth_token;
      if (form.base_url) toSave.base_url = form.base_url;
      if (form.model) toSave.model = form.model;
      if (form.effort) toSave.effort = form.effort;

      await setAgentConfig(toSave);
      setSaved(true);
      setTimeout(() => setSaved(false), 2500);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }, [form, setError]);

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
            {/* Auth Token */}
            <Field
              label="Auth Token"
              hint="Your API key. Stored locally in ~/.config/loopdeck/config.yaml."
            >
              <div className="relative">
                <Input
                  type={showKey ? "text" : "password"}
                  value={form.auth_token ?? ""}
                  onChange={(e) => handleChange("auth_token", e.target.value)}
                  placeholder="sk-abc123..."
                  className="pr-9 font-mono"
                />
                <button
                  type="button"
                  onClick={() => setShowKey((s) => !s)}
                  title={showKey ? "Hide token" : "Show token"}
                  className="absolute right-2 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded text-muted-foreground transition-colors hover:text-foreground"
                >
                  {showKey ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                </button>
              </div>
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

            {/* Model + Effort row */}
            <div className="grid grid-cols-2 gap-4">
              <Field label="Model" hint="Model ID. Pick a preset or type your own.">
                <Select
                  value={form.model ?? ""}
                  onValueChange={(v) => handleChange("model", v === "__custom__" ? "" : v)}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue placeholder="Select a model…" />
                  </SelectTrigger>
                  <SelectContent>
                    {MODEL_PRESETS.map((m) => (
                      <SelectItem key={m} value={m}>
                        {m}
                      </SelectItem>
                    ))}
                    <SelectItem value="__custom__">Custom (type below)</SelectItem>
                  </SelectContent>
                </Select>
                {/* Free-text override when "Custom" is selected, or when the
                    current value isn't one of the presets. */}
                {(form.model ?? "") === "" ||
                !MODEL_PRESETS.includes(form.model as (typeof MODEL_PRESETS)[number]) ? (
                  <Input
                    type="text"
                    value={form.model ?? ""}
                    onChange={(e) => handleChange("model", e.target.value)}
                    placeholder="claude-sonnet-4-6"
                    className="mt-2"
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
        </div>
      </div>
    </div>
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
