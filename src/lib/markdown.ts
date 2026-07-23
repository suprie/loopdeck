/**
 * Shared markdown helpers for spec-file rendering.
 *
 * Spec files (epic READMEs + PRDs) are stored as YAML frontmatter + prose
 * body. The frontmatter is metadata and should be stripped before passing
 * the body to {@link Markdown} for rendering.
 */

/** Strip the leading `---\n…\n---` YAML frontmatter from a spec file. */
export function stripFrontmatter(content: string): string {
  if (!content.startsWith("---\n")) return content;
  const after = content.slice(4);
  const end = after.indexOf("\n---\n");
  if (end === -1) return content;
  return after.slice(end + 5);
}
