import { useEffect } from "react";
import { useAppStore } from "./store/appStore";
import { useProjects } from "./hooks/useProjects";
import { AppShell } from "./components/layout/AppShell";
import { Dashboard } from "./components/dashboard/Dashboard";
import { ImportFlow } from "./components/import/ImportFlow";
import { ProjectDetail } from "./components/detail/ProjectDetail";
import { Settings } from "./components/settings/Settings";
import "./styles.css";

export default function App() {
  const currentView = useAppStore((s) => s.currentView);
  const loading = useAppStore((s) => s.isLoading);
  const { loadProjects } = useProjects();

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  return (
    <AppShell>
      {loading && currentView === "dashboard" ? null : null}
      {currentView === "dashboard" && <Dashboard />}
      {currentView === "import" && <ImportFlow />}
      {currentView === "detail" && <ProjectDetail />}
      {currentView === "settings" && <Settings />}
    </AppShell>
  );
}
