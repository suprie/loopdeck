import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Folder, File as FileIcon, ChevronRight, Loader2 } from "lucide-react";
import type { DirEntry } from "../../types";
import { listDirEntries, searchProjectFiles } from "../../lib/tauri";

// ── Types ────────────────────────────────────────────────────────────────────

/** Where the mention popup should anchor, in coordinates local to the
 *  textarea's offset parent (the relative-positioned wrapper around the
 *  composer). The hook converts raw textarea-local px into these. */
export interface CaretPosition {
  top: number;
  left: number;
  /** The textarea's rendered line height — used to drop the popup just below
   *  the caret line. */
  lineHeight: number;
}

/** A detected `@`-mention span in the draft, with the parsed segments used to
 *  decide what directory to list and how to filter it.
 *
 *  - `at` / `end` are absolute offsets into the draft string.
 *  - `subdir` is the directory path to list (everything before the final `/`
 *    in the token, or `""` for the project root).
 *  - `filter` is the trailing path segment being typed — the client-side
 *    substring filter applied to the fetched entries' names.
 *
 *  Examples (draft slice shown):
 *    `@`            → { subdir: "",     filter: "" }
 *    `@src`         → { subdir: "",     filter: "src" }
 *    `@src/`        → { subdir: "src",  filter: "" }
 *    `@src/comp`    → { subdir: "src",  filter: "comp" }
 *    `@src/comp/d/` → { subdir: "src/comp/d", filter: "" }
 */
interface MentionSpan {
  at: number;
  end: number;
  subdir: string;
  filter: string;
}

export interface UseFileMentionArgs {
  projectPath: string;
  /** Current full draft text. */
  draft: string;
  /** Current caret position (selectionStart) in the draft. */
  caret: number;
  /** The textarea element — needed to compute caret pixel coordinates. */
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  /** Setter for the draft; receives the next full string. */
  setDraft: (next: string) => void;
  /** Called with the new caret index after an insert, so the caller can sync
   *  its caret-tracking state. */
  setCaret: (next: number) => void;
  /** Disable the feature entirely (e.g. composer disabled / read-only). */
  disabled?: boolean;
}

export interface UseFileMentionResult {
  /** Whether the popup is currently open. */
  open: boolean;
  /** The filtered list currently shown. */
  items: DirEntry[];
  /** Currently-highlighted row index (arrow-key navigation). */
  selectedIndex: number;
  /** Whether a fetch is in flight. */
  loading: boolean;
  /** The mention span currently active, if any. */
  span: MentionSpan | null;
  /** Anchor coordinates for the popup (local to the composer wrapper). */
  position: CaretPosition | null;
  /** Pick the entry at `index` (or the highlighted one). Folders drill in,
   *  files insert and close. No-op if the list is empty. */
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
 * Parse the draft at the caret for an active `@`-mention span.
 *
 * Walks left from the caret collecting non-whitespace characters; if that run
 * begins with `@` whose `@` sits at string start or right after whitespace, we
 * have an active mention. The collected token (minus the `@`) is then split on
 * the last `/` to derive `subdir` + `filter`.
 */
function detectMention(draft: string, caret: number): MentionSpan | null {
  let i = caret;
  while (i > 0) {
    const ch = draft[i - 1];
    if (/\s/.test(ch)) break; // mention tokens can't cross whitespace
    i--;
  }
  // The token now spans [i, caret). It's a mention iff it starts with `@` and
  // that `@` is at string start or preceded by whitespace.
  if (draft[i] !== "@") return null;
  if (i > 0 && !/\s/.test(draft[i - 1])) return null;

  const token = draft.slice(i + 1, caret); // drop the `@`
  const slashIdx = token.lastIndexOf("/");
  const subdir = slashIdx >= 0 ? token.slice(0, slashIdx) : "";
  const filter = slashIdx >= 0 ? token.slice(slashIdx + 1) : token;
  return { at: i, end: caret, subdir, filter };
}

/**
 * Compute the caret's pixel offset within a `<textarea>` using the mirror-div
 * technique.
 *
 * Browsers don't expose `selectionStart` pixel coordinates, so we clone the
 * textarea's box model into a hidden div, replay the text up to the caret with
 * a marker span, and read the marker's bounding rect. This is the standard
 * approach used by mention libraries (slate, tribute, etc.).
 *
 * Returns null on any failure — callers fall back to anchoring at the
 * textarea's top-left, which is acceptable (popup still appears near the
 * composer).
 */
export function caretCoords(
  textarea: HTMLTextAreaElement,
  caret: number,
): CaretPosition | null {
  const style = window.getComputedStyle(textarea);

  // Build a mirror div that replicates the textarea's box model (font,
  // padding, border, width, wrapping). Replaying the text up to the caret
  // into it lets us read where the caret lands. Rebuilt per call — cheap, and
  // avoids stale-style bugs if the textarea's classes change (e.g. focus ring).
  const mirror = document.createElement("div");
  const props: (keyof CSSStyleDeclaration)[] = [
    "boxSizing",
    "width",
    "height",
    "overflowX",
    "overflowY",
    "borderTopWidth",
    "borderRightWidth",
    "borderBottomWidth",
    "borderLeftWidth",
    "borderStyle",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
    "fontStyle",
    "fontVariant",
    "fontWeight",
    "fontStretch",
    "fontSize",
    "fontSizeAdjust",
    "lineHeight",
    "fontFamily",
    "textAlign",
    "textTransform",
    "textIndent",
    "textDecoration",
    "letterSpacing",
    "wordSpacing",
    "tabSize",
  ];
  for (const p of props) {
    // @ts-expect-error — index by string; CSSStyleDeclaration allows it.
    mirror.style[p] = style[p];
  }
  mirror.style.position = "absolute"; // off the layout flow
  mirror.style.visibility = "hidden";
  mirror.style.whiteSpace = "pre-wrap";
  mirror.style.wordWrap = "break-word";
  // Anchored at the document origin. We only ever read offsetTop/offsetLeft
  // of the marker (relative to the mirror), so the mirror's own screen
  // position is irrelevant — this just keeps it out of view.
  mirror.style.top = "0";
  mirror.style.left = "0";

  // Replay text-before-caret, a marker, then text-after-caret. The marker is
  // display:inline-block so it has a measurable offsetTop/offsetLeft relative
  // to the mirror — those offsets ARE the caret's coordinates within the
  // textarea's content area, because the mirror replicates the textarea's box
  // model. (Using getBoundingClientRect on the marker would instead give
  // viewport coords, which require subtracting the textarea's viewport rect —
  // a step that breaks when the mirror is positioned away from the textarea.)
  const value = textarea.value.slice(0, caret);
  mirror.textContent = value;
  const marker = document.createElement("span");
  marker.style.display = "inline-block";
  marker.textContent = "\u200b"; // zero-width space — gives the span size
  mirror.appendChild(marker);

  document.body.appendChild(mirror);
  let result: CaretPosition | null = null;
  try {
    // offsetTop/offsetLeft are relative to offsetParent's *padding edge*
    // (the mirror here). The mirror replicates the textarea's box model, so
    // these give the caret's offset within the textarea's *content area*.
    // The popup, however, is positioned within the composer row whose
    // `top:0` is the textarea's *border-box* top — so add the textarea's
    // border + top padding to translate into that coordinate space. Without
    // this the popup would sit a few px too high (overlap the caret line).
    const top =
      marker.offsetTop +
      (parseFloat(style.borderTopWidth) || 0) +
      (parseFloat(style.paddingTop) || 0);
    const left = marker.offsetLeft;
    const lineHeight = parseFloat(style.lineHeight) || 20;
    result = { top, left, lineHeight };
  } catch {
    result = null;
  } finally {
    mirror.remove();
  }
  return result;
}

// ── Hook ─────────────────────────────────────────────────────────────────────

/**
 * Drives the `@`-mention autocomplete lifecycle: trigger detection, debounced
 * directory fetches (with an in-memory cache keyed by subdir), client-side
 * filtering, drill-in/insert, and keyboard navigation.
 *
 * The hook is intentionally unopinionated about the textarea's onChange/caret
 * wiring — the caller owns `draft`/`caret`/`setDraft`/`setCaret`, so this fits
 * alongside existing composer state without a competing source of truth.
 */
export function useFileMention({
  projectPath,
  draft,
  caret,
  textareaRef,
  setDraft,
  setCaret,
  disabled,
}: UseFileMentionArgs): UseFileMentionResult {
  const [span, setSpan] = useState<MentionSpan | null>(null);
  const [allEntries, setAllEntries] = useState<DirEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [position, setPosition] = useState<CaretPosition | null>(null);

  // Per-session cache of listed directories. Keyed by subdir; survives across
  // opens so re-typing `@src/` is instant on the second pass. Not cleared on
  // close — the FS rarely changes mid-composer-session, and a stale entry just
  // means one extra row the user can scroll past.
  const cacheRef = useRef<Map<string, DirEntry[]>>(new Map());
  // Token used to ignore stale fetch responses (debounced re-entries).
  const fetchTokenRef = useRef(0);
  // Debounce timer handle.
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const close = useCallback(() => {
    setSpan(null);
    setAllEntries([]);
    setLoading(false);
    setSelectedIndex(0);
    setPosition(null);
  }, []);

  // ── Trigger detection: re-parse the mention span on every draft/caret
  //    change. This is cheap (a single leftward scan) and keeps the popup in
  //    sync with edits (typing, deleting, arrowing around). ──
  useEffect(() => {
    if (disabled) {
      close();
      return;
    }
    const detected = detectMention(draft, caret);
    if (!detected) {
      // No active mention — tear down without clobbering position state we
      // don't need to (it'll be recomputed next open).
      if (span !== null) {
        setSpan(null);
        setAllEntries([]);
        setLoading(false);
        setSelectedIndex(0);
      }
      return;
    }
    // Recompute the caret anchor whenever the span is (re)opened or the caret
    // moves — this is what keeps the popup glued to the caret line.
    if (textareaRef.current) {
      const pos = caretCoords(textareaRef.current, caret);
      setPosition(pos);
    }
    setSpan(detected);
    setSelectedIndex(0);
  }, [draft, caret, disabled, textareaRef, span, close]);

  // ── Fetch + debounce. Two modes, picked by whether the user has typed a
  //    filter after the `@`:
  //      • No filter  (`@`, `@src/`)  → list ONE folder's children (browse/
  //        drill-in mode). Cached by subdir.
  //      • Filter set  (`@cha`, `@src/cha`) → recursive SEARCH across the whole
  //        project for the filter text. Cached by filter. This is what makes
  //        the autocomplete find files nested deep in the tree without the
  //        user having to drill folder-by-folder.
  //    The cache is keyed by a composite (`browse:<subdir>` / `search:<q>`) so
  //    the two modes never share a slot. ──
  useEffect(() => {
    if (!span) return;
    const subdir = span.subdir;
    const filter = span.filter;
    const mode = filter ? "search" : "browse";
    const cacheKey = mode === "search" ? `search:${filter}` : `browse:${subdir}`;

    // Cache hit — show immediately, no loading state.
    const cached = cacheRef.current.get(cacheKey);
    if (cached) {
      setAllEntries(cached);
      setLoading(false);
      setSelectedIndex(0);
      return;
    }

    // Debounce so a fast typist (`@src/com…`) doesn't fire one IPC per
    // keystroke. Search is heavier (full-tree walk), so debounce a touch longer
    // than browse to coalesce rapid typing.
    if (debounceRef.current) clearTimeout(debounceRef.current);
    setLoading(true);
    const token = ++fetchTokenRef.current;
    debounceRef.current = setTimeout(async () => {
      try {
        const entries =
          mode === "search"
            ? await searchProjectFiles(projectPath, filter, 50)
            : await listDirEntries(projectPath, subdir);
        // Ignore if a newer fetch superseded this one or the popup closed.
        if (token !== fetchTokenRef.current) return;
        cacheRef.current.set(cacheKey, entries);
        setAllEntries(entries);
      } catch {
        // Swallow — the popup just shows empty. We don't clear the cache slot
        // so a transient error doesn't retry forever on every keystroke.
        setAllEntries([]);
      } finally {
        if (token === fetchTokenRef.current) setLoading(false);
      }
    }, mode === "search" ? 160 : 120);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [projectPath, span?.subdir, span?.filter]); // eslint-disable-line react-hooks/exhaustive-deps

  // In search mode the backend already filtered + ranked, so show the results
  // as-is. In browse mode there's no client filter (filter is empty by
  // definition here), so this is just `allEntries`.
  const items = useMemo(() => {
    if (!span) return [];
    return allEntries;
  }, [allEntries, span]);

  // Keep the highlight in range as the filtered list shrinks/grows.
  useEffect(() => {
    setSelectedIndex((i) => (items.length === 0 ? 0 : Math.min(i, items.length - 1)));
  }, [items]);

  /**
   * Apply a mention selection: replace the `@…` span in the draft with the
   * chosen entry's relative path (prefixed with `@`). Folders append a
   * trailing `/` and keep the popup open (drill-in); files insert the bare
   * path and close. The caret lands just after the inserted text.
   */
  const apply = useCallback(
    (entry: DirEntry, drillIn: boolean) => {
      if (!span) return;
      const before = draft.slice(0, span.at);
      const after = draft.slice(span.end);
      let insert: string;
      if (drillIn) {
        // Drill-in: extend the token so the next fetch lists this folder. The
        // trailing `/` makes detectMention treat the post-slash text as the
        // filter of the new subdir.
        insert = `@${entry.path}/`;
      } else {
        // Insert: a finished file reference, with a trailing space so the user
        // can keep typing prose after it.
        insert = `@${entry.path} `;
      }
      const next = before + insert + after;
      const nextCaret = (before + insert).length;
      setDraft(next);
      setCaret(nextCaret);
      if (!drillIn) close();
    },
    [span, draft, setDraft, setCaret, close],
  );

  const pick = useCallback(
    (index?: number) => {
      const i = index ?? selectedIndex;
      const entry = items[i];
      if (!entry) return;
      apply(entry, entry.isDir);
    },
    [items, selectedIndex, apply],
  );

  /**
   * Keyboard navigation. Returns true if the event was consumed so the caller
   * skips its own onKeyDown (notably Enter-to-send). Only intercepts keys when
   * the popup is open and there are items.
   *
   *  ↑/↓    — move highlight
   *  Enter  — insert the highlighted entry (file) / drill in (folder)
   *  Tab/→  — drill in if the highlighted entry is a folder
   *  Esc    — close the popup
   */
  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>): boolean => {
      if (!span || items.length === 0) {
        // Esc still closes even with no items (e.g. while loading).
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
        case "Enter": {
          // MUST consume this — otherwise the composer's Enter-to-send fires
          // and the message is sent mid-mention.
          e.preventDefault();
          pick();
          return true;
        }
        case "Tab":
        case "ArrowRight": {
          const entry = items[selectedIndex];
          if (entry?.isDir) {
            e.preventDefault();
            apply(entry, true);
            return true;
          }
          return false;
        }
        case "Escape":
          e.preventDefault();
          close();
          return true;
        default:
          return false;
      }
    },
    [span, items, selectedIndex, pick, apply, close],
  );

  return {
    open: !!span,
    items,
    selectedIndex,
    loading,
    span,
    position,
    pick,
    close,
    onKeyDown,
    setSelectedIndex,
  };
}

// ── Component ────────────────────────────────────────────────────────────────

export interface FileMentionMenuProps {
  open: boolean;
  items: DirEntry[];
  loading: boolean;
  selectedIndex: number;
  position: CaretPosition | null;
  onHover: (index: number) => void;
  onPick: (index: number) => void;
}

/**
 * The mention popup itself — a caret-anchored, absolutely-positioned list of
 * files/folders. Rendered inside the composer's `relative` wrapper so the
 * `top`/`left` from `caretCoords` (which are local to the textarea) line up.
 *
 * Mouse interactions use `onMouseDown` + `preventDefault` rather than
 * `onClick`: a click would blur the textarea first (focus leaves the textarea
 * on mousedown), which would close the popup via the trigger-detection effect
 * before the click could register. Preventing default on mousedown keeps focus
 * in the textarea.
 */
export function FileMentionMenu({
  open,
  items,
  loading,
  selectedIndex,
  position,
  onHover,
  onPick,
}: FileMentionMenuProps) {
  if (!open) return null;

  // Float the popup ABOVE the composer row. Anchoring `bottom: 100%` puts the
  // popup's bottom edge exactly at the parent (relative) row's top edge — i.e.
  // the textbox's top — and the `+ 4px` gap lifts it just clear so it never
  // overlaps the textbox. `position`/caret tracking is no longer needed for
  // vertical placement now that the popup is full-width above the whole row.
  void position;

  return (
    <div
      // Full-width: `inset-x-0` stretches across the whole composer row
      // (textarea + send button), matching the reference screenshot's look.
      className="absolute inset-x-0 z-30 max-h-64 overflow-y-auto rounded-md border border-border bg-popover shadow-lg text-popover-foreground py-1"
      style={{ bottom: "calc(100% + 4px)" }}
      // Block mousedown so the textarea doesn't blur when interacting with the
      // popup (see the component doc comment).
      onMouseDown={(e) => e.preventDefault()}
    >
      {loading ? (
        <div className="flex items-center gap-2 px-3 py-3 text-xs text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" />
          <span>Loading…</span>
        </div>
      ) : items.length === 0 ? (
        <div className="px-3 py-3 text-xs text-muted-foreground text-center">
          No matching files or folders.
        </div>
      ) : (
        items.map((entry, i) => {
          const isSel = i === selectedIndex;
          const Icon = entry.isDir ? Folder : FileIcon;
          return (
            <div
              key={entry.path}
              // data-selected mirrors the shadcn CommandItem styling convention
              // so this list reads as native to the rest of the app's popovers.
              data-selected={isSel}
              onMouseEnter={() => onHover(i)}
              onClick={() => onPick(i)}
              className={`flex items-center gap-2 px-3 py-1.5 text-sm cursor-default select-none transition-colors ${
                isSel ? "bg-accent text-accent-foreground" : "text-foreground"
              }`}
            >
              <Icon
                className={`size-4 shrink-0 ${
                  // Distinct colors: folders use the primary (brand) color so
                  // they pop as navigable; files use a muted slate so folders
                  // visually dominate the list, like a typical file picker.
                  entry.isDir
                    ? "text-primary"
                    : "text-sky-500 dark:text-sky-400"
                }`}
              />
              <span
                className={`truncate flex-1 ${
                  // Folders get a slightly heavier weight to read as containers;
                  // files render in the default foreground.
                  entry.isDir ? "font-medium text-foreground" : "text-foreground/80"
                }`}
              >
                {entry.name}
              </span>
              {entry.isDir ? (
                <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
              ) : (
                <span className="text-[10px] text-muted-foreground truncate max-w-24">
                  {entry.path}
                </span>
              )}
            </div>
          );
        })
      )}
    </div>
  );
}
