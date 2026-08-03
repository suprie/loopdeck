import { useState } from "react";
import { Check, Eye, EyeOff, Loader2 } from "lucide-react";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import type { NamedAgentConfig, NamedAgentConfigInput } from "../../types";

const CLAUDE_EFFORT_OPTIONS = ["low", "medium", "high", "max"] as const;

export function emptyAgentConfig(): NamedAgentConfigInput {
  return {
    name: "",
    harness: "claude",
    auth_token: "",
    base_url: "",
    model: "",
    effort: "high",
  };
}

function editableProfile(profile: NamedAgentConfig): NamedAgentConfigInput {
  return {
    name: profile.name,
    harness: profile.harness ?? "claude",
    // Tokens are write-only. An empty edit preserves the profile's existing
    // token; typing a value replaces it on save.
    auth_token: "",
    base_url: profile.base_url ?? "",
    model: profile.model ?? "",
    effort: profile.effort ?? "",
  };
}

interface AgentConfigEditorProps {
  profile?: NamedAgentConfig;
  saving: boolean;
  onSave: (value: NamedAgentConfigInput) => Promise<void>;
  onCancel: () => void;
}

/** Focused editor shared by add and edit roster dialogs. */
export function AgentConfigEditor({
  profile,
  saving,
  onSave,
  onCancel,
}: AgentConfigEditorProps) {
  const [form, setForm] = useState<NamedAgentConfigInput>(
    profile ? editableProfile(profile) : emptyAgentConfig(),
  );
  const [showToken, setShowToken] = useState(false);

  const set = (field: keyof NamedAgentConfigInput, value: string) =>
    setForm((current) => ({ ...current, [field]: value }));

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    const name = form.name.trim();
    if (!name) return;
    const payload: NamedAgentConfigInput = { name, harness: form.harness };
    if (form.auth_token?.trim()) payload.auth_token = form.auth_token.trim();
    if (form.base_url?.trim()) payload.base_url = form.base_url.trim();
    if (form.model?.trim()) payload.model = form.model.trim();
    if (form.effort?.trim()) payload.effort = form.effort.trim();
    await onSave(payload);
  };

  const isCodex = form.harness === "codex";

  return (
    <form onSubmit={(event) => void handleSubmit(event)} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="agent-profile-name">Name</Label>
        <Input
          id="agent-profile-name"
          autoFocus
          value={form.name}
          onChange={(event) => set("name", event.target.value)}
          placeholder="e.g. Opus reviewer"
        />
      </div>

      <div className="space-y-1.5">
        <Label>Harness</Label>
        <Select
          value={form.harness ?? "claude"}
          onValueChange={(value) => {
            setForm((current) => ({
              ...current,
              harness: value as "claude" | "codex",
              effort: value === "codex" ? "" : current.effort || "high",
            }));
          }}
        >
          <SelectTrigger><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="claude">Claude Code</SelectItem>
            <SelectItem value="codex">Codex</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {isCodex ? (
        <p className="rounded-md border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          Codex uses the account and provider settings from your installed Codex CLI.
        </p>
      ) : (
        <div className="space-y-1.5">
          <Label htmlFor="agent-profile-token">Auth token</Label>
          <div className="relative">
            <Input
              id="agent-profile-token"
              type={showToken ? "text" : "password"}
              value={form.auth_token ?? ""}
              onChange={(event) => set("auth_token", event.target.value)}
              placeholder={profile?.has_auth_token ? "•••••••• (stored — type to replace)" : "Optional"}
              className="pr-9 font-mono"
            />
            <button
              type="button"
              onClick={() => setShowToken((visible) => !visible)}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              title={showToken ? "Hide token" : "Show token"}
            >
              {showToken ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </button>
          </div>
          {profile?.has_auth_token ? (
            <p className="inline-flex items-center gap-1 text-[11px] text-success">
              <Check className="size-3" /> Token stored locally
            </p>
          ) : null}
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label htmlFor="agent-profile-model">Model</Label>
          <Input
            id="agent-profile-model"
            value={form.model ?? ""}
            onChange={(event) => set("model", event.target.value)}
            placeholder="CLI default"
          />
        </div>
        <div className="space-y-1.5">
          <Label>Effort</Label>
          {isCodex ? (
            <Input
              value={form.effort ?? ""}
              onChange={(event) => set("effort", event.target.value)}
              placeholder="Model default"
            />
          ) : (
            <Select value={form.effort || "high"} onValueChange={(value) => set("effort", value)}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                {CLAUDE_EFFORT_OPTIONS.map((effort) => (
                  <SelectItem key={effort} value={effort}>{effort}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </div>
      </div>

      {!isCodex ? (
        <div className="space-y-1.5">
          <Label htmlFor="agent-profile-base-url">Base URL</Label>
          <Input
            id="agent-profile-base-url"
            value={form.base_url ?? ""}
            onChange={(event) => set("base_url", event.target.value)}
            placeholder="Provider default"
          />
        </div>
      ) : null}

      <div className="flex justify-end gap-2 pt-2">
        <button type="button" onClick={onCancel} className="h-9 rounded-md px-3 text-sm hover:bg-muted">
          Cancel
        </button>
        <button
          type="submit"
          disabled={saving || !form.name.trim()}
          className="inline-flex h-9 items-center gap-1.5 rounded-md bg-primary px-3.5 text-sm font-medium text-primary-foreground disabled:opacity-50"
        >
          {saving ? <Loader2 className="size-4 animate-spin" /> : null}
          {profile ? "Save profile" : "Add profile"}
        </button>
      </div>
    </form>
  );
}
