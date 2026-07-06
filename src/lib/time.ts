/**
 * Format an ISO 8601 date string as a relative time string.
 * e.g. "2h ago", "3d ago", "just now"
 */
export function relativeTime(isoString: string | null): string {
  if (!isoString) return "Uncommitted";

  const then = new Date(isoString).getTime();
  const now = Date.now();
  const diffMs = now - then;

  if (Number.isNaN(then)) return "Uncommitted";

  const seconds = Math.floor(diffMs / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  const weeks = Math.floor(days / 7);
  const months = Math.floor(days / 30);

  if (seconds < 60) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  if (hours < 24) return `${hours}h ago`;
  if (days < 7) return `${days}d ago`;
  if (weeks < 5) return `${weeks}w ago`;
  if (months < 12) return `${months}mo ago`;
  return new Date(isoString).toLocaleDateString();
}
