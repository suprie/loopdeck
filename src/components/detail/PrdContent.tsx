import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import * as api from "../../lib/tauri";
import { stripFrontmatter } from "../../lib/markdown";
import { Markdown } from "../shared/Markdown";

interface PrdContentProps {
  projectPath: string;
  /** Relative to docs/epics/ — e.g. "<slug>/prd-x.md". */
  relPath: string;
}

/**
 * Read-only renderer for a single PRD's full markdown body.
 *
 * Loads the raw spec file on mount via {@link api.readSpecFile}, strips the
 * YAML frontmatter, and renders the prose with the shared {@link Markdown}
 * primitive. Mounted lazily inside a Content tab, so a PRD is only fetched
 * when the user expands it.
 */
export function PrdContent({ projectPath, relPath }: PrdContentProps) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const raw = await api.readSpecFile(projectPath, relPath);
        if (!cancelled) {
          setContent(raw);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [projectPath, relPath]);

  if (error) {
    return (
      <p className="text-destructive text-xs px-1 py-2">
        Failed to load PRD: {error}
      </p>
    );
  }

  if (content === null) {
    return (
      <div className="flex items-center justify-center py-10 text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
      </div>
    );
  }

  return (
    <div className="max-h-[600px] overflow-y-auto px-1 py-2 text-sm">
      <Markdown>{stripFrontmatter(content)}</Markdown>
    </div>
  );
}
