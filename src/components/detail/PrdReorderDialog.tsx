import { useEffect, useRef, useState } from "react";
import { GripVertical, Loader2 } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "../ui/dialog";
import * as api from "../../lib/tauri";
import type { AppError, Prd } from "../../types";

interface PrdReorderDialogProps {
  projectPath: string;
  epicSlug: string;
  prds: Prd[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called after a successful save so the caller can refetch epics. */
  onReordered: () => void;
}

/**
 * Drag-to-reorder PRD list. Driven by plain pointer events (pointerdown /
 * pointermove / pointerup with pointer capture), not the HTML5 `draggable`
 * API — Tauri's native window-level drag-drop handling (used elsewhere in
 * the app for file attachments/import, see Chat.tsx / ImportFlow.tsx)
 * intercepts real OS drag gestures before they reach the webview, so
 * `dragover`/`drop` never fire for a `draggable` element inside a Tauri
 * window. Pointer events are ordinary DOM events and aren't touched by that.
 */
export function PrdReorderDialog({
  projectPath,
  epicSlug,
  prds,
  open,
  onOpenChange,
  onReordered,
}: PrdReorderDialogProps) {
  const [order, setOrder] = useState<Prd[]>(prds);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const rowRefs = useRef(new Map<string, HTMLLIElement>());

  // Reset local order to the current props whenever the dialog is (re)opened.
  // If any PRD has no explicit `order:` yet, start from filename order —
  // a stable, predictable baseline to drag from — rather than whatever
  // README/filename fallback happened to produce upstream.
  useEffect(() => {
    if (!open) return;
    const allOrdered = prds.every((p) => p.order != null);
    setOrder(allOrdered ? prds : [...prds].sort((a, b) => a.file.localeCompare(b.file)));
  }, [open, prds]);

  const handlePointerDown = (e: React.PointerEvent, index: number) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragIndex(index);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    if (dragIndex === null) return;
    const y = e.clientY;
    let targetIndex = dragIndex;
    for (let i = 0; i < order.length; i++) {
      const el = rowRefs.current.get(order[i].file);
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      if (y >= rect.top && y <= rect.bottom) {
        targetIndex = i;
        break;
      }
    }
    if (targetIndex !== dragIndex) {
      setOrder((prev) => {
        const next = [...prev];
        const [moved] = next.splice(dragIndex, 1);
        next.splice(targetIndex, 0, moved);
        return next;
      });
      setDragIndex(targetIndex);
    }
  };

  const handlePointerUp = (e: React.PointerEvent) => {
    e.currentTarget.releasePointerCapture(e.pointerId);
    setDragIndex(null);
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await api.setPrdOrder(projectPath, epicSlug, order.map((p) => p.file));
      toast.success("PRD order saved");
      onReordered();
      onOpenChange(false);
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to save order", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Reorder PRDs</DialogTitle>
          <DialogDescription>
            Drag to change delivery order. Saved as each PRD's `order:` field.
          </DialogDescription>
        </DialogHeader>

        <ul className="space-y-1.5">
          {order.map((prd, i) => (
            <li
              key={prd.file}
              ref={(el) => {
                if (el) rowRefs.current.set(prd.file, el);
                else rowRefs.current.delete(prd.file);
              }}
              className={`flex items-center gap-2 rounded-lg border border-border/60 bg-surface/40 px-3 py-2 text-xs transition-colors ${
                dragIndex === i ? "opacity-50" : ""
              }`}
            >
              <button
                type="button"
                onPointerDown={(e) => handlePointerDown(e, i)}
                onPointerMove={handlePointerMove}
                onPointerUp={handlePointerUp}
                className="shrink-0 touch-none cursor-grab text-muted-foreground/60 active:cursor-grabbing"
              >
                <GripVertical size={14} />
              </button>
              <span className="min-w-0 flex-1 truncate font-medium text-foreground">
                {prd.slug}
              </span>
              <span className="shrink-0 text-[10px] uppercase tracking-wider text-muted-foreground">
                {prd.status}
              </span>
            </li>
          ))}
        </ul>

        <DialogFooter>
          <button
            onClick={() => onOpenChange(false)}
            disabled={saving}
            className="h-8 rounded-md px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="flex h-8 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {saving && <Loader2 size={12} className="animate-spin" />}
            Save order
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
