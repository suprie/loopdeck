import { useNavigate } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { SpecEditor } from "../detail/SpecEditor";

interface SpecEditorPageProps {
  /** Relative to docs/epics/ — e.g. "<slug>/prd-x.md" or "<slug>/README.md". */
  relPath: string;
}

/**
 * Full-screen spec editor route.
 *
 * Wraps SpecEditor in a dedicated page with back navigation. The project path
 * comes from global app state (the active project selected in the sidebar).
 */
export function SpecEditorPage({ relPath }: SpecEditorPageProps) {
  const navigate = useNavigate();
  const selectedProject = useAppStore((s) =>
    s.selectedProjectPath ? s.projects.find((p) => p.path === s.selectedProjectPath) : null
  );
  const projectPath = selectedProject?.path;

  const filename = relPath.split("/").pop() || relPath;

  const handleSaved = () => {
    // Return to epics view after save.
    navigate({ to: "/epics" });
  };

  const handleCancel = () => {
    navigate({ to: "/epics" });
  };

  if (!projectPath) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-sm text-muted-foreground">No active project selected.</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header: back button + breadcrumb */}
      <div className="flex items-center gap-3 border-b border-border px-4 py-3">
        <button
          onClick={handleCancel}
          title="Back to epics"
          className="flex items-center gap-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground"
        >
          <ArrowLeft size={14} />
          Back
        </button>
        <span className="text-xs text-muted-foreground">/</span>
        <span className="font-mono text-xs text-muted-foreground">{relPath}</span>
      </div>

      {/* Full-height editor */}
      <div className="flex-1 overflow-hidden p-4">
        <SpecEditor
          projectPath={projectPath}
          relPath={relPath}
          filename={filename}
          onSaved={handleSaved}
          onCancel={handleCancel}
        />
      </div>
    </div>
  );
}
