import { cn } from "@/lib/utils";
import type { ProjectStatus } from "@/types";

/**
 * Shared status pill used by `ProjectCard` and `ProjectDetail`.
 *
 * The app's backend enum is `"active" | "warning" | "nonactive" | "archived"`
 * (note `nonactive`, not the clone's `inactive`); we map it here so the rest
 * of the codebase keeps using the established type.
 */
const STYLES: Record<ProjectStatus, { cls: string; dot: string; label: string }> = {
  active: {
    cls: "bg-success/10 text-success",
    dot: "bg-success",
    label: "Active",
  },
  warning: {
    cls: "bg-warning/10 text-warning",
    dot: "bg-warning",
    label: "Warning",
  },
  nonactive: {
    cls: "bg-destructive/10 text-destructive",
    dot: "bg-destructive",
    label: "Inactive",
  },
  archived: {
    cls: "bg-muted text-muted-foreground",
    dot: "bg-muted-foreground",
    label: "Archived",
  },
};

export function StatusBadge({
  status,
  className,
}: {
  status: ProjectStatus;
  className?: string;
}) {
  const s = STYLES[status] ?? STYLES.nonactive;
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center gap-1.5 rounded-full px-2 text-[10px] font-medium uppercase tracking-wider",
        s.cls,
        className,
      )}
    >
      <span className={cn("size-1.5 rounded-full", s.dot)} />
      {s.label}
    </span>
  );
}
