import { useState } from "react";
import { useProjects } from "../../hooks/useProjects";
import "./EditDescription.css";

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
    <div className="edit-description">
      <textarea
        className="edit-description__textarea"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Enter a project description..."
        autoFocus
      />
      <div className="edit-description__actions">
        <button className="btn-secondary" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn-primary" onClick={handleSave}>
          Save
        </button>
      </div>
    </div>
  );
}
