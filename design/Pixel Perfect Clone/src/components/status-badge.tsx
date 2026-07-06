import { cn } from "@/lib/utils";
import type { ProjectStatus } from "@/lib/mock-data";

const styles: Record<ProjectStatus, { cls: string; label: string }> = {
  active: { cls: "bg-success/10 text-success", label: "Active" },
  warning: { cls: "bg-warning/10 text-warning", label: "Warning" },
  inactive: { cls: "bg-destructive/10 text-destructive", label: "Inactive" },
  archived: { cls: "bg-muted text-muted-foreground", label: "Archived" },
};

export function StatusBadge({ status, className }: { status: ProjectStatus; className?: string }) {
  const s = styles[status];
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center gap-1.5 rounded-full px-2 text-[10px] font-medium uppercase tracking-wider",
        s.cls,
        className,
      )}
    >
      <span
        className={cn(
          "size-1.5 rounded-full",
          status === "active" && "bg-success",
          status === "warning" && "bg-warning",
          status === "inactive" && "bg-destructive",
          status === "archived" && "bg-muted-foreground",
        )}
      />
      {s.label}
    </span>
  );
}
