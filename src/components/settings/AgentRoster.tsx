import { useCallback, useEffect, useState } from "react";
import { Bot, Loader2, Pencil, Plus, Star, Trash2 } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import * as api from "../../lib/tauri";
import type { NamedAgentConfig, NamedAgentConfigInput } from "../../types";
import { ConfirmDialog } from "../shared/ConfirmDialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { AgentConfigEditor } from "./AgentConfigEditor";

function profileSummary(profile: NamedAgentConfig): string {
  const harness = profile.harness === "codex" ? "Codex" : "Claude Code";
  return [harness, profile.model || "CLI default", profile.effort || "default"].join(" · ");
}

/** Global profile manager. Assignments store profile IDs, never display names. */
export function AgentRoster() {
  const setError = useAppStore((state) => state.setError);
  const [profiles, setProfiles] = useState<NamedAgentConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [editor, setEditor] = useState<NamedAgentConfig | "new" | null>(null);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState<NamedAgentConfig | null>(null);
  const [settingDefault, setSettingDefault] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setProfiles(await api.listAgentConfigs());
    } catch (error) {
      setError(String(error));
    } finally {
      setLoading(false);
    }
  }, [setError]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = useCallback(async (input: NamedAgentConfigInput) => {
    setSaving(true);
    try {
      if (editor === "new") {
        await api.createAgentConfig(input);
      } else if (editor) {
        await api.updateAgentConfig(editor.id, input);
      }
      setEditor(null);
      await refresh();
    } catch (error) {
      setError(String(error));
    } finally {
      setSaving(false);
    }
  }, [editor, refresh, setError]);

  const makeDefault = useCallback(async (profile: NamedAgentConfig) => {
    setSettingDefault(profile.id);
    try {
      await api.setDefaultAgentConfig(profile.id);
      await refresh();
    } catch (error) {
      setError(String(error));
    } finally {
      setSettingDefault(null);
    }
  }, [refresh, setError]);

  const remove = useCallback(async () => {
    if (!deleting) return;
    try {
      await api.deleteAgentConfig(deleting.id);
      setDeleting(null);
      await refresh();
    } catch (error) {
      setError(String(error));
    }
  }, [deleting, refresh, setError]);

  return (
    <>
      <div className="mt-10 mb-4 flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          <Bot className="size-3.5" />
          Named agent profiles
        </div>
        <button
          onClick={() => setEditor("new")}
          className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-background px-2.5 text-xs font-medium hover:bg-muted"
        >
          <Plus className="size-3.5" /> Add profile
        </button>
      </div>

      <div className="rounded-xl border border-border bg-card shadow-[var(--shadow-sm)]">
        {loading ? (
          <div className="flex justify-center py-8"><Loader2 className="size-4 animate-spin text-muted-foreground" /></div>
        ) : profiles.length === 0 ? (
          <div className="px-6 py-7 text-center">
            <p className="text-sm font-medium">No named profiles yet</p>
            <p className="mt-1 text-xs text-muted-foreground">Add Claude or Codex profiles to assign them to a loop.</p>
          </div>
        ) : (
          <div className="divide-y divide-border">
            {profiles.map((profile) => (
              <div key={profile.id} className="flex items-center gap-3 px-5 py-4">
                <div className="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
                  <Bot className="size-4" />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <p className="truncate text-sm font-medium">{profile.name}</p>
                    {profile.is_default ? (
                      <span className="inline-flex items-center gap-1 rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                        <Star className="size-2.5 fill-current" /> Default
                      </span>
                    ) : null}
                  </div>
                  <p className="truncate text-xs text-muted-foreground">{profileSummary(profile)}</p>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {!profile.is_default ? (
                    <button
                      onClick={() => void makeDefault(profile)}
                      disabled={settingDefault === profile.id}
                      title="Make default"
                      className="inline-flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-50"
                    >
                      {settingDefault === profile.id ? <Loader2 className="size-3.5 animate-spin" /> : <Star className="size-3.5" />}
                    </button>
                  ) : null}
                  <button onClick={() => setEditor(profile)} title="Edit profile" className="inline-flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground">
                    <Pencil className="size-3.5" />
                  </button>
                  <button onClick={() => setDeleting(profile)} title="Delete profile" className="inline-flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-destructive/10 hover:text-destructive">
                    <Trash2 className="size-3.5" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <Dialog open={editor !== null} onOpenChange={(open) => !open && setEditor(null)}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>{editor === "new" ? "Add agent profile" : "Edit agent profile"}</DialogTitle>
            <DialogDescription>Profiles are global and can be assigned to any project loop.</DialogDescription>
          </DialogHeader>
          {editor !== null ? (
            <AgentConfigEditor
              profile={editor === "new" ? undefined : editor}
              saving={saving}
              onSave={save}
              onCancel={() => setEditor(null)}
            />
          ) : null}
        </DialogContent>
      </Dialog>

      {deleting ? (
        <ConfirmDialog
          title={`Delete ${deleting.name}?`}
          message="Existing run records keep their historical profile label, but this profile can no longer be assigned to new loops."
          confirmLabel="Delete profile"
          danger
          onConfirm={() => void remove()}
          onCancel={() => setDeleting(null)}
        />
      ) : null}
    </>
  );
}
