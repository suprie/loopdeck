import { useState } from "react";
import { useProjects } from "../../hooks/useProjects";

interface EditDescriptionProps {
  path: string;
  initialDescription: string;
  onSaved: () => void;
  onCancel: () => void;
}

export function EditDescription({
  path,
  initialDescription,
  onSaved,
  onCancel,
}: EditDescriptionProps) {
  const [value, setValue] = useState(initialDescription);
  const { updateDescription } = useProjects();

  const handleSave = async () => {
    await updateDescription(path, value);
    onSaved();
  };

  return (
    <div className="space-y-2">
      <textarea
        className="w-full min-h-[80px] px-3 py-2 rounded-md border border-border bg-surface text-sm text-foreground placeholder:text-muted-foreground resize-y focus:outline-none focus:border-primary/50 focus:ring-1 focus:ring-primary/30 font-sans"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Enter a project description..."
        autoFocus
      />
      <div className="flex gap-2">
        <button
          onClick={onCancel}
          className="inline-flex items-center h-7 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
        >
          Cancel
        </button>
        <button
          onClick={handleSave}
          className="inline-flex items-center h-7 px-3 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition"
        >
          Save
        </button>
      </div>
    </div>
  );
}
