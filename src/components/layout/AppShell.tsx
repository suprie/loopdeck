import type { ReactNode } from "react";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import "./AppShell.css";

interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps) {
  const error = useAppStore((s) => s.error);
  const setError = useAppStore((s) => s.setError);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const { scanFolder } = useProjects();

  const handleScan = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Folder to Scan",
      });
      if (selected && typeof selected === "string") {
        await scanFolder(selected);
      }
    } catch {
      // User cancelled or dialog failed
    }
  };

  return (
    <div className="app-shell">
      <header className="app-shell__header">
        <div className="app-shell__brand">LoopDeck</div>
        <div className="app-shell__actions">
          <button className="btn-secondary" onClick={() => setCurrentView("dashboard")}>
            Dashboard
          </button>
          <button className="btn-primary" onClick={handleScan}>
            Scan Folder
          </button>
        </div>
      </header>

      {error && (
        <div className="app-shell__error">
          <span>{error}</span>
          <button className="app-shell__error-dismiss" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}

      <main className="app-shell__content">{children}</main>
    </div>
  );
}
