import { useState } from "react";
import { Check, Eye, EyeOff, Loader2 } from "lucide-react";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Textarea } from "../ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import type { NamedAgentConfig, NamedAgentConfigInput, RoleCharter } from "../../types";

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

/** Charter form state: skills held as one comma-separated string for editing. */
interface CharterForm {
  persona_prompt: string;
  allowed_skills: string;
  output_contract: string;
}

function charterForm(profile?: NamedAgentConfig): CharterForm {
  return {
    persona_prompt: profile?.persona_prompt ?? "",
    allowed_skills: profile?.allowed_skills?.join(", ") ?? "",
    output_contract: profile?.output_contract ?? "",
  };
}

/** Trim editor input; empty fields are omitted so the backend clears them. */
function charterPayload(form: CharterForm): RoleCharter {
  const charter: RoleCharter = {};
  if (form.persona_prompt.trim()) charter.persona_prompt = form.persona_prompt.trim();
  const skills = form.allowed_skills
    .split(",")
    .map((skill) => skill.trim())
    .filter(Boolean);
  if (skills.length) charter.allowed_skills = skills;
  if (form.output_contract.trim()) charter.output_contract = form.output_contract.trim();
  return charter;
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
  onSave: (value: NamedAgentConfigInput, charter: RoleCharter) => Promise<void>;
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
  const [charter, setCharter] = useState<CharterForm>(charterForm(profile));
  const [showToken, setShowToken] = useState(false);

  const set = (field: keyof NamedAgentConfigInput, value: string) =>
    setForm((current) => ({ ...current, [field]: value }));

  const setCharterField = (field: keyof CharterForm, value: string) =>
    setCharter((current) => ({ ...current, [field]: value }));

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    const name = form.name.trim();
    if (!name) return;
    const payload: NamedAgentConfigInput = { name, harness: form.harness };
    if (form.auth_token?.trim()) payload.auth_token = form.auth_token.trim();
    if (form.base_url?.trim()) payload.base_url = form.base_url.trim();
    if (form.model?.trim()) payload.model = form.model.trim();
    if (form.effort?.trim()) payload.effort = form.effort.trim();
    await onSave(payload, charterPayload(charter));
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

      <div className="space-y-3 rounded-md border border-border bg-muted/20 p-3">
        <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Role charter (optional)
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="agent-charter-persona">Persona</Label>
          <Textarea
            id="agent-charter-persona"
            rows={2}
            value={charter.persona_prompt}
            onChange={(event) => setCharterField("persona_prompt", event.target.value)}
            placeholder="e.g. You are the QA agent: skeptical, evidence-driven, verify before trusting."
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="agent-charter-skills">Allowed skills</Label>
          <Input
            id="agent-charter-skills"
            value={charter.allowed_skills}
            onChange={(event) => setCharterField("allowed_skills", event.target.value)}
            placeholder="e.g. loopdeck-prd-verifier, loopdeck-open-pr"
          />
          <p className="text-[11px] text-muted-foreground">
            Comma-separated skill names. Suggested, not enforced.
          </p>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="agent-charter-contract">Output contract</Label>
          <Textarea
            id="agent-charter-contract"
            rows={2}
            value={charter.output_contract}
            onChange={(event) => setCharterField("output_contract", event.target.value)}
            placeholder="e.g. End every run with a verification verdict and the evidence behind it."
          />
        </div>
        <p className="text-[11px] text-muted-foreground">
          Advisory for now — charters shape prompts in a later phase and are not enforced.
        </p>
      </div>

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
