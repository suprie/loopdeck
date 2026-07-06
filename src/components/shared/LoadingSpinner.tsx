import { Loader2 } from "lucide-react";
import "./LoadingSpinner.css";

interface LoadingSpinnerProps {
  label?: string;
}

export function LoadingSpinner({ label = "Loading..." }: LoadingSpinnerProps) {
  return (
    <div className="loading-spinner">
      <Loader2 className="loading-spinner__icon" />
      <span className="loading-spinner__label">{label}</span>
    </div>
  );
}
