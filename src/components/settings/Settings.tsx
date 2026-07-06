import { useState, useEffect, useCallback } from "react";
import { Save, Loader2, Check, Settings2 } from "lucide-react";
import { PageHeader } from "../layout/AppShell";
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
    <div className="flex-1 flex flex-col min-h-0">
      <PageHeader
        title="Settings"
        subtitle="Configure your AI agent provider"
      />

      <div className="flex-1 min-h-0 overflow-y-auto px-6 py-6">
        <div className="max-w-xl space-y-8">
          {/* Agent Configuration Section */}
          <section>
            <div className="flex items-center gap-2 mb-5">
              <Settings2 className="size-4 text-muted-foreground" />
              <h2 className="text-sm font-semibold tracking-tight">
                Agent Configuration
              </h2>
            </div>

            <div className="space-y-4 bg-surface/40 rounded-lg border border-border p-5">
              {/* Auth Token */}
              <Field label="Auth Token" htmlFor="auth_token">
                <input
                  id="auth_token"
                  type="password"
                  value={form.auth_token ?? ""}
                  onChange={(e) => handleChange("auth_token", e.target.value)}
                  placeholder="sk-abc123..."
                  className="w-full h-9 px-3 rounded-md bg-background border border-input text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring transition"
                />
                <FieldHint>
                  Your API key. Stored locally in{" "}
                  <code className="text-[11px] bg-muted px-1 py-0.5 rounded">
                    ~/.config/loopdeck/config.yaml
                  </code>
                  .
                </FieldHint>
              </Field>

              {/* Base URL */}
              <Field label="Base URL" htmlFor="base_url">
                <input
                  id="base_url"
                  type="text"
                  value={form.base_url ?? ""}
                  onChange={(e) => handleChange("base_url", e.target.value)}
                  placeholder="https://api.anthropic.com"
                  className="w-full h-9 px-3 rounded-md bg-background border border-input text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring transition"
                />
                <FieldHint>
                  Provider endpoint. Leave blank for default Anthropic API.
                </FieldHint>
              </Field>

              {/* Model */}
              <Field label="Model" htmlFor="model">
                <input
                  id="model"
                  type="text"
                  value={form.model ?? ""}
                  onChange={(e) => handleChange("model", e.target.value)}
                  placeholder="claude-sonnet-4-6"
                  list="model-presets"
                  className="w-full h-9 px-3 rounded-md bg-background border border-input text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring transition"
                />
                <datalist id="model-presets">
                  {MODEL_PRESETS.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
                <FieldHint>
                  Model ID. Start typing or pick from presets via the datalist.
                </FieldHint>
              </Field>

              {/* Effort */}
              <Field label="Effort Level" htmlFor="effort">
                <select
                  id="effort"
                  value={form.effort ?? "high"}
                  onChange={(e) => handleChange("effort", e.target.value)}
                  className="w-full h-9 px-3 rounded-md bg-background border border-input text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-ring transition appearance-none"
                >
                  {EFFORT_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
                <FieldHint>
                  Higher effort = more thorough reasoning, slower responses.
                </FieldHint>
              </Field>
            </div>
          </section>

          {/* Save */}
          <div className="flex items-center gap-3">
            <button
              onClick={handleSave}
              disabled={saving}
              className="inline-flex items-center gap-2 h-9 px-4 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:opacity-90 disabled:opacity-50 transition-opacity"
            >
              {saving ? (
                <>
                  <Loader2 className="size-4 animate-spin" />
                  Saving…
                </>
              ) : saved ? (
                <>
                  <Check className="size-4" />
                  Saved
                </>
              ) : (
                <>
                  <Save className="size-4" />
                  Save Configuration
                </>
              )}
            </button>

            {saved && (
              <span className="text-xs text-success">
                Configuration saved successfully.
              </span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Small reusable form primitives ──────────────────────────────────────────

function Field({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label
        htmlFor={htmlFor}
        className="block text-xs font-medium text-muted-foreground mb-1.5"
      >
        {label}
      </label>
      {children}
    </div>
  );
}

function FieldHint({ children }: { children: React.ReactNode }) {
  return (
    <p className="text-[11px] text-muted-foreground mt-1.5 leading-relaxed">
      {children}
    </p>
  );
}
