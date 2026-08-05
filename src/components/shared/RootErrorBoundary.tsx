import { Component, Fragment, useState, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";

interface RootErrorBoundaryProps {
  children: ReactNode;
}

interface RootErrorBoundaryState {
  /** The caught error, or `null` while the tree is healthy. */
  error: Error | null;
  /** Bumped on "Try again" to force a remount of the child subtree via `key`. */
  resetKey: number;
}

/**
 * Top-level React error boundary.
 *
 * Mounted above `<App>` in `main.tsx` so a render-time crash anywhere in the
 * app tree — `ThemeProvider`, `RouterProvider`, `Toaster`, or any view
 * component — surfaces a recoverable fallback instead of a blank white screen.
 * The router's own `errorComponent` (`router.tsx`) only catches errors thrown
 * *inside* a route; this boundary catches the pre-router and shell-setup
 * failures it can't reach.
 *
 * Recovery is best-effort:
 * - **Reload app** — `window.location.reload()`; guaranteed recovery. Selasar
 *   is offline-first, so on-disk data (`.loopdeck/`, `config.yaml`, the
 *   local `agent_token` file) survives a full reload untouched.
 * - **Try again** — clears the boundary and remounts the subtree (the bumped
 *   `resetKey` is applied as a `Fragment` `key`). This drops transient React
 *   state and re-runs render; a *deterministic* crash will trip the boundary
 *   again immediately, leaving the user no worse off and pointed at Reload.
 *
 * Note: React error boundaries catch render / lifecycle / constructor errors
 * only — not async, event-handler, or `useEffect`-callback errors. Those are
 * surfaced through the app's existing error banner (`appStore.error`) and
 * toasts; this boundary is the last line of defense for the render path.
 *
 * The fallback is intentionally self-contained: no Tauri, router, theme, or
 * store dependencies, so it renders even when the providers it normally wraps
 * are the thing that crashed. It relies only on the base OKLCH tokens declared
 * in `:root` (`styles.css`), which are present without `ThemeProvider`.
 */
export class RootErrorBoundary extends Component<
  RootErrorBoundaryProps,
  RootErrorBoundaryState
> {
  state: RootErrorBoundaryState = { error: null, resetKey: 0 };

  static getDerivedStateFromError(error: Error): Partial<RootErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Surface to the devtools console / any future error reporter. Deliberately
    // not rethrown — the boundary owns the fallback.
    console.error("[RootErrorBoundary] Uncaught render error:", error, info);
  }

  private handleReload = () => {
    window.location.reload();
  };

  private handleTryAgain = () => {
    this.setState((prev) => ({ error: null, resetKey: prev.resetKey + 1 }));
  };

  render() {
    const { error, resetKey } = this.state;

    if (error) {
      return (
        <ErrorFallback
          error={error}
          onReload={this.handleReload}
          onTryAgain={this.handleTryAgain}
        />
      );
    }

    // A keyed Fragment remounts the whole app subtree on "Try again" without
    // adding an extra DOM node to the tree.
    return <Fragment key={resetKey}>{this.props.children}</Fragment>;
  }
}

function ErrorFallback({
  error,
  onReload,
  onTryAgain,
}: {
  error: Error;
  onReload: () => void;
  onTryAgain: () => void;
}) {
  const [showDetails, setShowDetails] = useState(false);

  return (
    <div className="flex h-screen w-full items-center justify-center bg-background px-4 text-foreground">
      <div className="w-full max-w-md">
        <div className="flex flex-col items-center text-center">
          <div className="flex size-12 items-center justify-center rounded-xl bg-[color-mix(in_oklab,var(--destructive)_15%,transparent)] text-destructive">
            <AlertTriangle className="size-6" />
          </div>
          <h1 className="mt-5 text-xl font-semibold tracking-tight">
            Selasar ran into a problem
          </h1>
          <p className="mt-2 text-sm text-muted-foreground">
            Something crashed while rendering the app. Your data is safe on disk —
            reloading restores everything.
          </p>

          <div className="mt-6 flex flex-wrap justify-center gap-2">
            <button
              type="button"
              onClick={onReload}
              className="inline-flex h-9 items-center justify-center gap-1.5 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
            >
              <RefreshCw className="size-4" />
              Reload app
            </button>
            <button
              type="button"
              onClick={onTryAgain}
              className="inline-flex h-9 items-center justify-center rounded-md border border-border bg-background px-4 text-sm font-medium text-foreground transition-colors hover:bg-accent"
            >
              Try again
            </button>
          </div>

          <button
            type="button"
            onClick={() => setShowDetails((v) => !v)}
            className="mt-4 text-xs text-muted-foreground transition-colors hover:text-foreground"
          >
            {showDetails ? "Hide" : "Show"} error details
          </button>

          {showDetails && (
            <pre className="mt-3 max-h-48 w-full overflow-auto rounded-md border border-border bg-surface p-3 text-left font-mono text-xs text-muted-foreground">
              {error.message}
              {error.stack ? `\n\n${error.stack}` : ""}
            </pre>
          )}
        </div>
      </div>
    </div>
  );
}
