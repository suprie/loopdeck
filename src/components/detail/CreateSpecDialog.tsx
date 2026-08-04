import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "../ui/dialog";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Textarea } from "../ui/textarea";
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from "../ui/select";
import * as api from "../../lib/tauri";
import { slugify } from "../../lib/utils";
import type { AppError, Epic } from "../../types";

interface CreateSpecDialogProps {
  projectPath: string;
  epics: Epic[];
  mode: "epic" | "prd";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (relPath: string) => void;
}

/** Quote a value for YAML frontmatter (double-quoted scalar, escaped quotes).
 *  Newlines/whitespace are collapsed — a raw newline inside double quotes is an
 *  invalid flow scalar and would make serde_yaml skip the whole file. */
function yaml(value: string): string {
  const single = value.replace(/\s+/g, " ").trim();
  return `"${single.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

/** Epic README skeleton — all 5 required frontmatter fields. `milestone` is
 *  quoted so `0.1.0` doesn't parse as a float and get skipped by serde_yaml. */
function epicSkeleton(title: string, slug: string, milestone: string, description: string): string {
  return `---
title: ${yaml(title)}
slug: ${slug}
milestone: ${yaml(milestone || "0.1.0")}
status: proposed
description: ${yaml(description)}
---

# ${title}

## Summary

...

## Goals

-

## Non-Goals

-
`;
}

/** PRD skeleton — all 4 required frontmatter fields. The phase loop carries a
 *  stable `` `prd-slug/loop-slug` `` ID so it can be promoted once authored. */
function prdSkeleton(
  prdSlug: string,
  epicSlug: string,
  epicMilestone: string,
  title: string,
  description: string,
): string {
  return `---
prd: ${prdSlug}
epic: ${epicSlug}
milestone: ${yaml(epicMilestone || "0.1.0")}
status: proposed
description: ${yaml(description)}
---

# ${title}

## Summary

...

## Phases

### Phase 1 — Foundation

- [ ] \`${prdSlug}/${prdSlug}-kickoff\` Kick off the work
`;
}

/**
 * Create an epic or PRD from a small form, then hand the new spec file to the
 * caller to open in the editor. `write_spec_file` is a raw overwrite — no
 * existence check — so collisions are guarded here against the already-loaded
 * epics before writing.
 */
export function CreateSpecDialog({
  projectPath,
  epics,
  mode,
  open,
  onOpenChange,
  onCreated,
}: CreateSpecDialogProps) {
  const [title, setTitle] = useState("");
  const [milestone, setMilestone] = useState("0.1.0");
  const [description, setDescription] = useState("");
  const [selectedEpic, setSelectedEpic] = useState("");
  const [creating, setCreating] = useState(false);

  // Fresh form each open; PRD defaults its epic to the first one.
  useEffect(() => {
    if (open) {
      setTitle("");
      setMilestone("0.1.0");
      setDescription("");
      setSelectedEpic(epics[0]?.slug ?? "");
      setCreating(false);
    }
  }, [open, epics]);

  const slug = slugify(title);
  const canCreate = slug.length > 0 && !creating && (mode === "epic" || selectedEpic);

  const handleCreate = async () => {
    if (!canCreate) return;
    setCreating(true);
    try {
      if (mode === "epic") {
        if (epics.some((e) => e.slug === slug)) {
          toast.error("Epic already exists", {
            description: `docs/epics/${slug}/README.md is already authored.`,
          });
          return;
        }
        const relPath = `${slug}/README.md`;
        await api.writeSpecFile(
          projectPath,
          relPath,
          epicSkeleton(title, slug, milestone, description),
        );
        onCreated(relPath);
      } else {
        const epic = epics.find((e) => e.slug === selectedEpic);
        if (!epic) return;
        const prdSlug = `prd-${slug}`;
        if (epic.prds.some((p) => p.file === `${prdSlug}.md` || p.slug === prdSlug)) {
          toast.error("PRD already exists", {
            description: `docs/epics/${epic.slug}/${prdSlug}.md is already authored.`,
          });
          return;
        }
        const relPath = `${epic.slug}/${prdSlug}.md`;
        await api.writeSpecFile(
          projectPath,
          relPath,
          prdSkeleton(prdSlug, epic.slug, epic.milestone ?? "", title, description),
        );
        onCreated(relPath);
      }
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to create", { description: appErr.message ?? String(err) });
    } finally {
      setCreating(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{mode === "epic" ? "New epic" : "New PRD"}</DialogTitle>
          <DialogDescription>
            {mode === "epic"
              ? "Writes docs/epics/<slug>/README.md with a reviewable skeleton."
              : "Writes docs/epics/<epic>/prd-<slug>.md with a first phase + loop."}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-1">
          {mode === "prd" && (
            <div className="space-y-1.5">
              <Label>Epic</Label>
              <Select value={selectedEpic} onValueChange={setSelectedEpic}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Choose an epic" />
                </SelectTrigger>
                <SelectContent>
                  {epics.map((e) => (
                    <SelectItem key={e.slug} value={e.slug}>
                      {e.title}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          <div className="space-y-1.5">
            <Label>Title</Label>
            <Input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={mode === "epic" ? "e.g. Support Project Management" : "e.g. Structured Execution State"}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleCreate();
              }}
            />
            {slug && <p className="font-mono text-[10px] text-muted-foreground">/{slug}</p>}
          </div>

          {mode === "epic" && (
            <div className="space-y-1.5">
              <Label>Milestone</Label>
              <Input
                value={milestone}
                onChange={(e) => setMilestone(e.target.value)}
                placeholder="0.1.0"
                className="font-mono text-[11px]"
              />
            </div>
          )}

          <div className="space-y-1.5">
            <Label>Description</Label>
            <Textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="One-line summary of the goal…"
              rows={2}
              className="resize-none"
            />
          </div>
        </div>

        <DialogFooter>
          <button
            type="button"
            onClick={() => onOpenChange(false)}
            className="inline-flex h-9 items-center rounded-md bg-muted px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => void handleCreate()}
            disabled={!canCreate}
            className="inline-flex h-9 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {creating && <Loader2 className="size-3.5 animate-spin" />}
            Create
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
