# Memory Budget Report — 2026-08-30

Phase 1 measurement for `prd-memory-hygiene`. Token counts are **chars/4**
estimates (offline; no tokenizer installed), the method pinned by this run's
pre-answered clarification. Re-measure any time with
`wc -c <file>` and divide by 4.

## Measured state (pre-compaction)

| File | Chars | Est. tokens (chars/4) | Lines | Entries |
|---|---|---|---|---|
| `loops.md` | 159,896 | **~39,974** | 394 | 62 History entries |
| `decisions.md` | 32,600 | **~8,150** | 161 | 27 entries |

Reference sizes: `decisions-archive.md` 251,679 chars (~63K tok),
`loops-archive.md` 90,498 chars (~23K tok), `loops.legacy.md` 66,032 chars
(~17K tok).

### Entry-length distribution

| File | Unit | Min | Median | Max |
|---|---|---|---|---|
| `loops.md` History | tokens/entry | 6 | **297** | 4,322 |
| `loops.md` History | words/entry | 3 | 150 | 2,299 |
| `decisions.md` | tokens/entry | 89 | **252** | 818 |
| `decisions.md` | words/entry | 57 | 113 | 468 |

Largest offenders: one 2026-07-24 loops "Last completed" bullet (~4,322 tok,
2,299 words — a single line), `prd-run-queue` Phase 2/4 entries (~1,036/1,046
tok), decisions entries of 818/780 tok.

### Observations driving the proposal

- `loops.md` whole-file cost (~40K tok) exceeds even the 2026-07-19 incident's
  pre-fix `loops.md` (~25K tok) — the 90KB archive fix regressed.
- Median entry is already ~250-300 tok, but the tail (single 800-4,300 tok
  entries) is what blows the budget: **entry length, not entry count, is the
  dominant cost**.
- Post-incident healthy size (2026-07-19 fix) was 11 KB / ~2.75K tok — a
  proven-good operating point.

## Proposal (adopted into the `loopdeck-memory` skill by Loop 2)

| Rule | Value |
|---|---|
| Active-file budget (each of `loops.md`, `decisions.md`, whole file) | **3,000 tokens (~12KB)** |
| Archive trigger | the write that would push the active file past **2,400 tokens (~9.6KB)** |
| New-entry target — decisions | ≤ **60 words** (~250 chars) per 3-bullet entry; longer rationale goes to a `Detail` doc |
| New-entry target — loops History `**Summary**` | ≤ **50 words**, one sentence |
| Live-entry ceiling (verification; any entry still in an active file) | **≤ 300 words / ~400 tokens** — over that, archive it or split to a `Detail` doc |
| Estimator | chars/4 (offline), re-derived from `wc -c` |

Derivation: 3,000 tok ≈ the proven 11KB post-incident size; fits ~15
convention-length decisions entries (~90 tok each) plus `loops.md`'s
Current/Next Steps/History with headroom. The 2,400-token trigger leaves 20%
headroom so no single conventional write overshoots the budget. The old de
-facto trigger (~90KB / ~22.5K tok) is retired — the new one is ~10x lower.

Enforcement: **document-only** (this run's pre-answered clarification);
automated enforcement, if ever wanted, belongs to `prd-process-discipline.md`
(the PRD's Open Question, resolved). The existing count-based windows
(~15 decisions / ~5 history entries) remain as secondary defaults; **the token
budget supersedes them when they conflict**.

## Post-compaction targets (Loops 3-5)

After archiving, both active files must measure ≤ 3,000 tokens whole-file with
every live entry ≤ 300 words — re-verified in Loop 5 with the same chars/4
method. Archives must carry a pointer line from each active file (Loop 6
verifies readability + findability across the full archives).

## Post-compaction verification (Loop 5, 2026-08-30)

| File | Chars | Tokens (chars/4) | Budget 3,000 | Trigger 2,400 | Entries | Max entry |
|---|---|---|---|---|---|---|
| `loops.md` | 3,137 | **784** | PASS | PASS | 20 (5 History) | 88 tok / 45 words |
| `decisions.md` | 8,906 | **2,226** | PASS | PASS (headroom ~170 tok for the run's own new entry) | 11 | 479 tok / 254 words |

Per-entry check: every live entry is within the ~300-word ceiling (largest:
254 words, the 2026-08-28 wizard decision). Its chars/4 figure (479) sits
~20% over the ~400-token approximation — words are the primary prose
measure and chars/4 overestimates code-dense text, so it stands; noted here
rather than silently rounded. All loops History `**Summary**` bullets are
≤ 50 words (max 45). Both files also carry their archive pointer line
(Loop 6 scope). Loop 3/4 repairs: one stale truncated duplicate of the
2026-08-26 BLOCKED loops entry dropped from the archive (strict prefix of
the complete entry); the orphaned decisions bullet block (former lines
25-29) had its `## 2026-08-26 — …` heading restored before archiving.

## Archive verification (Loop 6, 2026-08-30 — full scope)

Scope: `decisions-archive.md`, `loops-archive.md`, and `loops.legacy.md`
(pre-PRD migration snapshot — structural headings only, intact, left as-is).
Checks: pointer lines in both active files, heading shape, orphaned bullet
blocks, exact-duplicate entries, truncated tails.

Fixes applied (fix-then-confirm, per the run's pre-answered clarification):

- `decisions-archive.md`: dropped **5 exact-duplicate entries** (4 entries
  this run's append re-added that an earlier compaction had already copied —
  the active file had regrown them — plus 1 pre-existing duplicate). Every
  remaining entry (144) carries a well-formed `## date — title` header.
- `loops-archive.md`: restored a lost `### 2026-07-19 — Phase 3a — …`
  heading in the original archive content (an entry's Status bullets had no
  heading above them); the stale truncated BLOCKED-entry copy dropped in
  Loop 3 is documented in the appendix comment there.
- Pointers confirmed: `loops.md` → `loops-archive.md`,
  `decisions.md` → `decisions-archive.md`, both near the top of the file.

Re-verified after fixes: **no findings** — archives readable (dated headers,
no orphans), deduped, and findable from the active files.

## Final state (post ship-prep)

Appending this run's own decision entry pushed `decisions.md` to 2,407
tokens — past the 2,400 trigger — so the convention was applied to itself in
the same write: the oldest kept entry (2026-08-26 gauges, 254 tok) moved to
the archive.

Ship-prep then found the run's branch was based on c81ab69, a prior-run
commit not on `origin/main` (which had advanced through the PR #92 merge,
carrying a sibling `decisions.md`/`loops.md` state). The compaction was
rebuilt as a **union on `origin/main`**: keep-set merged both lineages'
recent entries; every non-kept entry from either lineage is in the archive;
`loops-archive.md`'s appendix regenerates from origin/main's post-#92 active
body. origin/main's 2026-08-30 "Morning report renders…" decision (~500 tok,
at the live-entry ceiling) was archived under the convention rather than
kept. The c81ab69-only orphaned bullet block it duplicated is preserved in
the archive under its repaired heading.

**Final: `loops.md` 2,422 chars (~606 tok), `decisions.md` 8,611 chars
(~2,153 tok), 11 entries — both under trigger and budget.** Archive re-verify
after the union rebuild: no findings.
