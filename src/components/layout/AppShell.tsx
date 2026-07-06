import type { ReactNode } from "react";

/**
 * Reusable page header rendered at the top of most views.
 *
 * NOTE: The main app shell (sidebar + content area) is now defined in
 * `src/router.tsx` as `AppShellLayout`, the root route component.  This file
 * only exports `PageHeader` so existing view imports don't need to change.
 */
export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="h-14 px-6 border-b border-border flex items-center justify-between bg-background/80 backdrop-blur sticky top-0 z-10 flex-shrink-0">
      <div>
        <h1 className="text-sm font-semibold tracking-tight">{title}</h1>
        {subtitle && (
          <p className="text-xs text-muted-foreground">{subtitle}</p>
        )}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </header>
  );
}
