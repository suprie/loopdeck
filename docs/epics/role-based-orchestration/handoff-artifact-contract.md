---
title: Handoff Artifact Contract
epic: role-based-orchestration
prd: prd-handoff-spike
status: draft (spike deliverable, reviewable)
created: 2026-09-01
---

# Handoff Artifact Contract

This is the contract `prd-handoff-spike` Loop 1 was scoped to produce. It
defines what a handoff artifact **is** (schema), how **big** it may be (size
caps), and what "consumed" **measurably** means (citation rule). It resolves
the spike PRD's Open Question #1: the format is **Markdown + YAML
frontmatter** — frontmatter carries the machine-readable record (author role,
phase, type), the Markdown body carries the content.

`prd-agent-handoff` implements against this contract; the spike's findings
(its Design section) record how well two real sessions obeyed it.

## 1. Location and format

| Rule | Value |
|---|---|
| Path | `.loopdeck/handoffs/<topic>.md` |
| `<topic>` | kebab-case slug, stable once written (consumer prompts reference it) |
| Format | YAML frontmatter block, then Markdown body |
| Encoding | UTF-8, LF line endings |

One artifact = one topic = one file. Append-only in spirit: once a consumer
has been pointed at an artifact, headings and item IDs are never renamed —
add new items instead.

## 2. Frontmatter schema (the artifact record)

Every field except `cites` is required.

| Field | Type | Constraints |
|---|---|---|
| `artifact` | string | the `<topic>` slug, matches filename |
| `author_role` | string | role slug of the writing session, e.g. `business-analyst` |
| `phase` | string | originating phase/run identifier |
| `type` | enum | `plan` \| `analysis` \| `decision` \| `report` \| `content` |
| `created` | date | `YYYY-MM-DD` |
| `summary` | string | ≤ 200 chars, one line |
| `cites` | list | optional: paths of upstream artifacts this one consumed |

Example:

```yaml
---
artifact: session-export
author_role: business-analyst
phase: handoff-spike/two-agent-run
type: plan
created: 2026-09-01
summary: One-click export of the selected session transcript to a Markdown file.
cites: []
---
```

## 3. Body schema

Stable top-level headings. Items inside the content sections carry **stable
IDs** so citation is unambiguous:

| Heading | Required | Item IDs |
|---|---|---|
| `## Summary` | yes | — |
| `## Requirements` | yes for `plan`/`analysis` | `R1`, `R2`, … |
| `## Constraints` | no | `C1`, `C2`, … |
| `## Non-Goals` | yes for `plan` | — |
| `## Open Questions` | no | `Q1`, `Q2`, … |

Rules:

- Max one level of nesting under a heading; no deeper structure.
- Each numbered item is a single claim, ≤ 400 characters.
- IDs are assigned once and never reused after deletion.

## 4. Size caps

| Cap | Value | Enforcement |
|---|---|---|
| Frontmatter | ≤ 1 KiB | producer |
| Body soft cap | 4 KiB | producer should restructure (split the topic) |
| Body hard cap | 8 KiB | consumer treats oversized artifact as a producer bug |
| Top-level sections | ≤ 8 | producer |
| Numbered items (R+C+Q) | ≤ 12 | producer |

Rationale: `prd-agent-handoff` P0 requires downstream injection to be
**bounded in size** — the cap is chosen for prompt injection, not disk. An
artifact over the hard cap must be reported back (see §6), never silently
truncated by the consumer.

## 5. Citation rule — what "consumed" measurably means

The consumer's output **must** contain a `## Handoff citations` block listing
every part of the artifact it used, referenced by stable ID:
`<path>#<Heading>` for sections, `<path>#<Rn>` / `#<Cn>` / `#<Qn>` for items.

An artifact counts as **consumed** when all three hold:

1. **Coverage** — every section heading and every numbered item in the
   artifact appears in the citations block, either as cited or as an explicit
   `not-used: <id> — <one-line reason>`.
2. **Fidelity** — no consumer claim contradicts a cited item, and no
   requirement is fabricated and attributed to the artifact.
3. **Completeness** — paraphrase preserves item boundaries: silently
   dropping, merging, or halving items is **truncation** and fails the check.

Spike go/no-go bar (single run): **GO** requires all three; a **NO-GO**
records the specific failure mode (ignored input, drift/fabrication, or
truncation).

## 6. Producer and consumer obligations

**Producer (upstream role session):**

- Write only within this schema and the §4 caps.
- Emit the artifact before declaring the phase complete.
- If the topic outgrows the caps, split into a second artifact and `cites`
  the first.

**Consumer (downstream role session):**

- Read the artifact from the path given in the prompt — the prompt names the
  path, never the content.
- Cite per §5; an artifact section that is genuinely irrelevant gets an
   explicit `not-used` line, not silence.
- If the artifact is missing, over the hard cap, or self-contradictory:
   stop and report that — do not improvise a replacement.
