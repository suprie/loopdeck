import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Sparkles, Loader2 } from "lucide-react";
import type { SkillEntry } from "../../types";
import { listSkills } from "../../lib/tauri";

// ── Types ────────────────────────────────────────────────────────────────────

/** A detected `/`-skill span in the draft, with the text typed after the `/`
 *  — the client-side substring filter applied to the fetched skills' names and
 *  descriptions.
 *
 *  - `at` / `end` are absolute offsets into the draft string.
 *  - `query` is the token after the leading `/`, lowercased for matching.
 *
 *  Examples (draft slice shown):
 *    `/`        → { query: "" }
 *    `/rust`    → { query: "rust" }
 *    `/loopdeck` → { query: "loopdeck" }
 */
interface SkillSpan {
  at: number;
  end: number;
  query: string;
}

export interface UseSkillDiscoveryArgs {
  projectPath: string;
  /** Current full draft text. */
  draft: string;
  /** Current caret position (selectionStart) in the draft. */
  caret: number;
  /** Setter for the draft; receives the next full string. */
  setDraft: (next: string) => void;
  /** Called with the new caret index after an insert, so the caller can sync
   *  its caret-tracking state. */
  setCaret: (next: number) => void;
  /** Disable the feature entirely (e.g. composer disabled / read-only). */
  disabled?: boolean;
}

export interface UseSkillDiscoveryResult {
  /** Whether the popup is currently open. */
  open: boolean;
  /** The filtered list currently shown. */
  items: SkillEntry[];
  /** Currently-highlighted row index (arrow-key navigation). */
  selectedIndex: number;
  /** Whether a fetch is in flight. */
  loading: boolean;
  /** Pick the skill at `index` (or the highlighted one). No-op if the list is
   *  empty. */
  pick: (index?: number) => void;
  /** Close the popup (Esc / blur / insert). */
  close: () => void;
  /** Keyboard handler to call from the textarea's onKeyDown. Returns true if
   *  the event was consumed (caller should NOT run its own handler). */
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /** Set the highlighted index (used by hover). */
  setSelectedIndex: (i: number) => void;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Parse the draft at the caret for an active `/`-skill span.
 *
 * Walks left from the caret collecting non-whitespace characters; if that run
 * begins with `/` whose `/` sits at string start or right after whitespace, we
 * have an active skill span. This mirrors `detectMention`'s boundary rule so a
 * prose path like `src/foo` (where the `/` is mid-token) never triggers.
 */
function detectSkill(draft: string, caret: number): SkillSpan | null {
  let i = caret;
  while (i > 0) {
    const ch = draft[i - 1];
    if (/\s/.test(ch)) break; // skill tokens can't cross whitespace
    i--;
  }
  // The token now spans [i, caret). It's a skill span iff it starts with `/`
  // and that `/` is at string start or preceded by whitespace.
  if (draft[i] !== "/") return null;
  if (i > 0 && !/\s/.test(draft[i - 1])) return null;

  const query = draft.slice(i + 1, caret); // drop the `/`
  return { at: i, end: caret, query };
}

// ── Hook ─────────────────────────────────────────────────────────────────────

/**
 * Drives the `/`-skill discovery lifecycle: trigger detection, a single fetch
 * of the project's installed skills (cached per session), client-side
 * filtering as the user types, insert, and keyboard navigation.
 *
 * Simpler than `useFileMention`: skills are a flat list with no folder drill-in,
 * so there's no browse/search duality and no debounce — we fetch once on first
 * open and filter in-memory on every keystroke. The cache survives across
 * opens for the session; skills rarely change mid-composer.
 *
 * Like `useFileMention`, the hook is unopinionated about the textarea's
 * onChange/caret wiring — the caller owns `draft`/`caret`/`setDraft`/`setCaret`.
 */
export function useSkillDiscovery({
  projectPath,
  draft,
  caret,
  setDraft,
  setCaret,
  disabled,
}: UseSkillDiscoveryArgs): UseSkillDiscoveryResult {
  const [span, setSpan] = useState<SkillSpan | null>(null);
  const [allSkills, setAllSkills] = useState<SkillEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);

  // Per-session cache of the project's skills. Keyed by projectPath so a
  // project switch refetches; survives across opens otherwise. Cleared never
  // mid-session — skills are written at project bootstrap and don't move.
  const cacheRef = useRef<Map<string, SkillEntry[]>>(new Map());
  // Token used to ignore a stale fetch if the user closes/reopens quickly.
  const fetchTokenRef = useRef(0);

  const close = useCallback(() => {
    setSpan(null);
    setLoading(false);
    setSelectedIndex(0);
  }, []);

  // ── Trigger detection: re-parse the skill span on every draft/caret change.
  //    Cheap (a single leftward scan) and keeps the popup in sync with edits. ──
  useEffect(() => {
    if (disabled) {
      close();
      return;
    }
    const detected = detectSkill(draft, caret);
    if (!detected) {
      if (span !== null) {
        setSpan(null);
        setLoading(false);
        setSelectedIndex(0);
      }
      return;
    }
    setSpan(detected);
    setSelectedIndex(0);
  }, [draft, caret, disabled, span, close]);

  // ── Fetch the skill list once per open (cache by projectPath). The fetch is
  //    not debounced — it's a single directory read of a handful of small
  //    SKILL.md files, so there's nothing to coalesce. Filtering happens
  //    client-side in the `items` memo below. ──
  useEffect(() => {
    if (!span) return;

    const cached = cacheRef.current.get(projectPath);
    if (cached) {
      setAllSkills(cached);
      setLoading(false);
      return;
    }

    setLoading(true);
    const token = ++fetchTokenRef.current;
    (async () => {
      try {
        const skills = await listSkills(projectPath);
        // Ignore if a newer fetch superseded this one or the popup closed.
        if (token !== fetchTokenRef.current) return;
        cacheRef.current.set(projectPath, skills);
        setAllSkills(skills);
      } catch {
        // Swallow — the popup just shows empty. We don't cache the failure so a
        // transient error retries on the next open.
        if (token === fetchTokenRef.current) setAllSkills([]);
      } finally {
        if (token === fetchTokenRef.current) setLoading(false);
      }
    })();
  }, [span, projectPath]); // eslint-disable-line react-hooks/exhaustive-deps

  // Client-side substring filter on name + description. Lowercased query
  // matched against the lowercased fields; empty query shows everything.
  const items = useMemo(() => {
    if (!span) return [];
    const q = span.query.toLowerCase();
    if (!q) return allSkills;
    return allSkills.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q),
    );
  }, [allSkills, span]);

  // Keep the highlight in range as the filtered list shrinks/grows.
  useEffect(() => {
    setSelectedIndex((i) => (items.length === 0 ? 0 : Math.min(i, items.length - 1)));
  }, [items]);

  /**
   * Apply a skill selection: replace the `/…` span in the draft with the
   * chosen skill's invocation token (`/<name>`) followed by a trailing space
   * so the user can keep typing the prompt. The caret lands just after the
   * inserted text. Uses the frontmatter `name` (e.g. `loopdeck:rust-expert`),
   * which is what the `claude` CLI invokes the skill by.
   */
  const apply = useCallback(
    (skill: SkillEntry) => {
      if (!span) return;
      const before = draft.slice(0, span.at);
      const after = draft.slice(span.end);
      const insert = `/${skill.name} `;
      const next = before + insert + after;
      const nextCaret = (before + insert).length;
      setDraft(next);
      setCaret(nextCaret);
      close();
    },
    [span, draft, setDraft, setCaret, close],
  );

  const pick = useCallback(
    (index?: number) => {
      const i = index ?? selectedIndex;
      const skill = items[i];
      if (!skill) return;
      apply(skill);
    },
    [items, selectedIndex, apply],
  );

  /**
   * Keyboard navigation. Returns true if the event was consumed so the caller
   * skips its own onKeyDown (notably Enter-to-send). Only intercepts keys when
   * the popup is open and there are items.
   *
   *  ↑/↓    — move highlight
   *  Enter/Tab — insert the highlighted skill
   *  Esc    — close the popup
   */
  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>): boolean => {
      if (!span || items.length === 0) {
        // Esc still closes even with no items (e.g. while loading, or a project
        // with no skills installed).
        if (e.key === "Escape" && span) {
          close();
          return true;
        }
        return false;
      }
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setSelectedIndex((i) => (i + 1) % items.length);
          return true;
        case "ArrowUp":
          e.preventDefault();
          setSelectedIndex((i) => (i - 1 + items.length) % items.length);
          return true;
        case "Enter":
        case "Tab":
          // MUST consume Enter — otherwise the composer's Enter-to-send fires
          // and the message is sent mid-selection.
          e.preventDefault();
          pick();
          return true;
        case "Escape":
          e.preventDefault();
          close();
          return true;
        default:
          return false;
      }
    },
    [span, items, pick, close],
  );

  return {
    open: !!span,
    items,
    selectedIndex,
    loading,
    pick,
    close,
    onKeyDown,
    setSelectedIndex,
  };
}

// ── Component ────────────────────────────────────────────────────────────────

export interface SkillMenuProps {
  open: boolean;
  items: SkillEntry[];
  loading: boolean;
  selectedIndex: number;
  onHover: (index: number) => void;
  onPick: (index: number) => void;
}

/**
 * The skill-discovery popup — an absolutely-positioned list of the project's
 * installed skills, anchored above the composer row. Rendered inside the
 * composer's `relative` wrapper (same anchoring scheme as `FileMentionMenu`):
 * `bottom: calc(100% + 4px)` floats it just clear of the textbox.
 *
 * Mouse interactions use `onMouseDown` + `preventDefault` rather than
 * `onClick`: a click would blur the textarea first (focus leaves the textarea
 * on mousedown), which would close the popup via the trigger-detection effect
 * before the click could register. Preventing default on mousedown keeps focus
 * in the textarea.
 */
export function SkillMenu({
  open,
  items,
  loading,
  selectedIndex,
  onHover,
  onPick,
}: SkillMenuProps) {
  if (!open) return null;

  return (
    <div
      className="absolute inset-x-0 z-30 max-h-64 overflow-y-auto rounded-md border border-border bg-popover shadow-lg text-popover-foreground py-1"
      style={{ bottom: "calc(100% + 4px)" }}
      // Block mousedown so the textarea doesn't blur when interacting with the
      // popup (see the component doc comment).
      onMouseDown={(e) => e.preventDefault()}
    >
      {loading ? (
        <div className="flex items-center gap-2 px-3 py-3 text-xs text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" />
          <span>Loading skills…</span>
        </div>
      ) : items.length === 0 ? (
        <div className="px-3 py-3 text-xs text-muted-foreground text-center">
          No skills installed for this project.
        </div>
      ) : (
        items.map((skill, i) => {
          const isSel = i === selectedIndex;
          return (
            <div
              key={skill.directory}
              // data-selected mirrors the shadcn CommandItem styling convention
              // so this list reads as native to the rest of the app's popovers.
              data-selected={isSel}
              onMouseEnter={() => onHover(i)}
              onClick={() => onPick(i)}
              className={`flex items-start gap-2 px-3 py-1.5 text-sm cursor-default select-none transition-colors ${
                isSel ? "bg-accent text-accent-foreground" : "text-foreground"
              }`}
            >
              <Sparkles
                className={`size-4 shrink-0 mt-0.5 ${
                  isSel ? "text-primary" : "text-primary/70"
                }`}
              />
              <div className="flex flex-col min-w-0 flex-1">
                <div className="flex items-center gap-1.5 min-w-0">
                  <span className="font-medium text-foreground truncate">
                    {skill.name}
                  </span>
                  {skill.argumentHint && (
                    // A dimmed placeholder chip cueing what to type after the
                    // skill name (e.g. `<prd-file-path>` for the orchestrator).
                    // Purely informational — picking still inserts `/name ` and
                    // leaves the caret ready to type the argument as plain text.
                    <span
                      className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-mono ${
                        isSel
                          ? "bg-primary/15 text-primary"
                          : "bg-muted text-muted-foreground"
                      }`}
                    >
                      {skill.argumentHint}
                    </span>
                  )}
                </div>
                {skill.description && (
                  <span className="text-xs text-muted-foreground truncate">
                    {skill.description}
                  </span>
                )}
              </div>
            </div>
          );
        })
      )}
    </div>
  );
}
