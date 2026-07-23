import { useState, useEffect } from "react";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import * as api from "../../lib/tauri";
import { stripFrontmatter } from "../../lib/markdown";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "../ui/tabs";
import { Markdown } from "../shared/Markdown";

interface SpecEditorProps {
  projectPath: string;
  /** Relative to docs/epics/ — e.g. "<slug>/prd-x.md" or "<slug>/README.md". */
  relPath: string;
  /** Filename to show in the header (e.g. "prd-spec-layer.md"). */
  filename: string;
  onSaved: () => void;
  onCancel: () => void;
}

/**
 * In-place markdown editor for spec files (epic READMEs + PRDs).
 *
 * Write/Preview tabs compose the existing Textarea + Markdown primitives — no
 * new editor dependency. Loads the raw file on mount, saves on demand.
 */
export function SpecEditor({
  projectPath,
  relPath,
  filename,
  onSaved,
  onCancel,
}: SpecEditorProps) {
  const [content, setContent] = useState("");
  const [original, setOriginal] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const raw = await api.readSpecFile(projectPath, relPath);
        if (!cancelled) {
          setContent(raw);
          setOriginal(raw);
        }
      } catch (err) {
        if (!cancelled) toast.error("Failed to load file", { description: String(err) });
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [projectPath, relPath]);

  const dirty = content !== original;

  const handleSave = async () => {
    setSaving(true);
    try {
      await api.writeSpecFile(projectPath, relPath, content);
      setOriginal(content);
      toast.success("Saved", { description: filename });
      onSaved();
    } catch (err) {
      toast.error("Failed to save", { description: String(err) });
    } finally {
      setSaving(false);
    }
  };

  const handleCancel = () => {
    if (dirty && !window.confirm("Discard unsaved changes?")) return;
    onCancel();
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12 text-muted-foreground">
        <Loader2 className="size-5 animate-spin" />
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-surface">
      {/* Header: filename + tabs */}
      <div className="flex items-center justify-between border-b border-border px-3 py-2">
        <span className="font-mono text-[11px] text-muted-foreground">{filename}</span>
        <Tabs defaultValue="write">
          <TabsList className="h-7">
            <TabsTrigger value="write" className="px-2.5 py-0.5 text-[11px]">
              Write
            </TabsTrigger>
            <TabsTrigger value="preview" className="px-2.5 py-0.5 text-[11px]">
              Preview
            </TabsTrigger>
          </TabsList>

          {/* Write */}
          <TabsContent value="write">
            <textarea
              className="w-full min-h-[400px] resize-y bg-transparent px-3 py-2.5 font-mono text-xs leading-relaxed text-foreground outline-none"
              value={content}
              onChange={(e) => setContent(e.target.value)}
              autoFocus
              spellCheck={false}
            />
          </TabsContent>

          {/* Preview */}
          <TabsContent value="preview">
            <div className="max-h-[500px] overflow-y-auto px-4 py-3">
              <Markdown>{stripFrontmatter(content)}</Markdown>
            </div>
          </TabsContent>
        </Tabs>
      </div>

      {/* Footer: save / cancel */}
      <div className="flex items-center justify-end gap-2 border-t border-border px-3 py-2">
        <button
          onClick={handleCancel}
          className="inline-flex items-center h-7 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
        >
          Cancel
        </button>
        <button
          onClick={handleSave}
          disabled={!dirty || saving}
          className="inline-flex items-center gap-1.5 h-7 px-3 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {saving && <Loader2 className="size-3 animate-spin" />}
          {dirty && !saving && <span className="size-1.5 rounded-full bg-primary-foreground/70" />}
          Save
        </button>
      </div>
    </div>
  );
}
