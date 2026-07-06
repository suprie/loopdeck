import { Loader2 } from "lucide-react";

interface LoadingSpinnerProps {
  label?: string;
}

export function LoadingSpinner({ label = "Loading..." }: LoadingSpinnerProps) {
  return (
    <div className="flex flex-col items-center justify-center py-12 gap-4 text-muted-foreground">
      <Loader2 className="size-8 animate-spin" />
      <span className="text-sm">{label}</span>
    </div>
  );
}
