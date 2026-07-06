import type { ReactNode } from "react";

/**
 * Reusable labelled section used inside the project detail overview card.
 * Matches the clone's anatomy: a `text-[10px]` uppercase tracking-wider label,
 * optional right-aligned actions, separated from neighbours by a hairline.
 */
export function Section({
  label,
  actions,
  children,
}: {
  label: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="border-b border-border py-4 first:pt-0 last:border-b-0 last:pb-0">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          {label}
        </span>
        {actions && <div className="flex gap-1">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

/** Compact icon-only button used inside `Section` actions. */
export function IconButton({
  children,
  label,
  onClick,
}: {
  children: ReactNode;
  label: string;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
    >
      {children}
    </button>
  );
}

/**
 * Row action button for the detail footer (Rescan / Finder / Terminal / Remove).
 * The `destructive` variant tints the icon and hover.
 */
export function ActionButton({
  icon: Icon,
  label,
  onClick,
  destructive,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  onClick?: () => void;
  destructive?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        "inline-flex h-8 items-center gap-1.5 rounded-md px-3 text-xs font-medium transition-colors " +
        (destructive
          ? "text-destructive hover:bg-destructive/10"
          : "text-muted-foreground hover:bg-accent hover:text-foreground")
      }
    >
      <Icon className="size-3.5" />
      {label}
    </button>
  );
}
