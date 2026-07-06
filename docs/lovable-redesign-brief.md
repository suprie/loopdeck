# LoopDeck — Visual Redesign Brief for Lovable

> Pair this document with the main Lovable prompt. It contains the design
> system tokens, component anatomy, and ASCII mockups for every key screen in
> **both light and dark mode**. Lovable reads ASCII layouts well — paste the
> relevant section with each iteration.

---

## 1. Design Tokens (OKLCH)

The codebase already uses OKLCH via `@theme inline` in `src/styles.css`.
Replace the single `:root` (dark-first) block with **two** complete palettes.

### Light mode (`:root`, default)

```
--background:           oklch(0.99 0.003 270)   /* near-white, faint cool */
--surface:              oklch(0.985 0.003 270)   /* sidebar, subtle step */
--surface-elevated:     oklch(1 0 0)             /* cards = pure white */
--card:                 oklch(1 0 0)
--card-foreground:      oklch(0.18 0.01 270)
--popover:              oklch(1 0 0)
--popover-foreground:   oklch(0.18 0.01 270)
--foreground:           oklch(0.18 0.01 270)     /* near-black text */

--muted:                oklch(0.96 0.004 270)    /* hover fills */
--muted-foreground:     oklch(0.5 0.012 270)     /* secondary text */
--accent:               oklch(0.96 0.006 295)    /* active nav bg */
--accent-foreground:    oklch(0.18 0.01 270)

--primary:              oklch(0.55 0.2 295)      /* violet — darker for AA contrast */
--primary-foreground:   oklch(0.99 0 0)
--secondary:            oklch(0.96 0.004 270)
--secondary-foreground: oklch(0.2 0.01 270)

--destructive:          oklch(0.58 0.22 25)
--destructive-foreground: oklch(0.99 0 0)
--success:              oklch(0.62 0.16 155)
--warning:              oklch(0.7 0.15 75)

--border:               oklch(0.93 0.004 270)    /* hairline */
--input:                oklch(0.95 0.004 270)
--ring:                 oklch(0.55 0.2 295)

--shadow-color:         220 40% 20%              /* for shadow mixins */
--shadow-sm:            0 1px 2px hsl(var(--shadow-color) / 0.04)
--shadow-md:            0 1px 2px hsl(var(--shadow-color) / 0.04), 0 4px 16px -8px hsl(var(--shadow-color) / 0.08)
--shadow-lg:            0 4px 24px -6px hsl(var(--shadow-color) / 0.12)
```

### Dark mode (`.dark`)

```
--background:           oklch(0.16 0.012 270)    /* keep current */
--surface:              oklch(0.185 0.013 270)
--surface-elevated:     oklch(0.21 0.014 270)
--card:                 oklch(0.195 0.013 270)
--card-foreground:      oklch(0.96 0.005 270)
--popover:              oklch(0.21 0.014 270)
--popover-foreground:   oklch(0.96 0.005 270)
--foreground:           oklch(0.96 0.005 270)

--muted:                oklch(0.24 0.014 270)
--muted-foreground:     oklch(0.66 0.015 270)
--accent:               oklch(0.26 0.02 285)
--accent-foreground:    oklch(0.96 0.005 270)

--primary:              oklch(0.72 0.17 295)     /* keep current violet */
--primary-foreground:   oklch(0.16 0.012 270)
--secondary:            oklch(0.26 0.015 270)
--secondary-foreground: oklch(0.96 0.005 270)

--destructive:          oklch(0.65 0.22 25)
--destructive-foreground: oklch(0.98 0 0)
--success:              oklch(0.74 0.16 155)
--warning:              oklch(0.78 0.15 75)

--border:               oklch(0.27 0.014 270)
--input:                oklch(0.26 0.014 270)
--ring:                 oklch(0.72 0.17 295)

/* Dark mode: no colored shadows — use borders + elevation for depth */
--shadow-sm:            none
--shadow-md:            0 0 0 1px var(--border)
--shadow-lg:            0 0 0 1px var(--border), 0 8px 32px -12px oklch(0 0 0 / 0.5)
```

### Type & radius (shared)

```
--radius:   0.625rem          /* keep — it's working */
--font-sans:    Inter, ui-sans-serif, system-ui
--font-mono:    "JetBrains Mono", ui-monospace

/* Type scale — tight, dev-tool feel */
text-[10px]  uppercase tracking-wider  →  section labels, meta
text-xs      (12px)                    →  body small, captions
text-sm      (14px)                    →  body, nav items
text-base    (16px)                    →  (rare, avoid in dense UI)
font-semibold tracking-tight           →  headings, brand
```

---

## 2. Component Anatomy

Build a `src/components/ui/` primitive set. Lovable should generate these first.

### Button

```
Variants:  primary | secondary | ghost | destructive | outline
Sizes:     sm (h-7 text-xs) | md (h-9 text-sm) | icon (size-9)

primary    → bg-primary text-primary-foreground hover:opacity-90
secondary  → bg-secondary text-secondary-foreground hover:bg-accent
ghost      → text-muted-foreground hover:bg-accent hover:text-foreground
outline    → border border-border bg-transparent hover:bg-accent
destructive → text-destructive hover:bg-destructive/10

All: rounded-md, font-medium, 150ms transition, focus-visible:ring-2 ring-ring ring-offset-2
```

### Card

```
rounded-xl border border-border bg-card p-5
light mode:  + shadow-sm
hover (interactive):  border-primary/40 + shadow-md, translateY(-1px)
```

### Badge / Status Pill

```
inline-flex items-center gap-1.5 rounded-full
h-5 px-2 text-[10px] font-medium uppercase tracking-wider

Statuses:
  active   → bg-success/10 text-success         • (filled dot)
  warning  → bg-warning/10 text-warning
  inactive → bg-destructive/10 text-destructive
  archived → bg-muted text-muted-foreground
```

### Nav Item (sidebar)

```
Default:   text-muted-foreground
Hover:     bg-accent/60 text-foreground
Active:    bg-accent text-foreground
           + 2px primary bar on the LEFT edge (inset) — Linear-style indicator
```

### Theme Toggle

```
Segmented control in the sidebar footer, 3 states:
  [ ☀️ Light | 🌓 Auto | 🌙 Dark ]
Active segment: bg-surface-elevated shadow-sm
Icons: Sun / Monitor / Moon from lucide
```

---

## 3. Screen Mockups

### 3.1 App Shell — Light Mode

```
┌─────────────────┬──────────────────────────────────────────────────────┐
│ ◆ LoopDeck      │  Dashboard                          [+ Import Repo] │
│ ENGINEERING     │  12 projects · 8 active                              │
│ COCKPIT         ├──────────────────────────────────────────────────────┤
│                 │                                                      │
│ ▸ Dashboard  ◀══│   ┌────────────┐  ┌────────────┐  ┌────────────┐    │
│   Activity       │   │ L  loopdeck │  │ A  agentui │  │ F  foobar  │    │
│   Agent Runner   │   │             │  │            │  │            │    │
│   Decisions      │   │ Desc line…  │  │ Desc line… │  │ Desc line… │    │
│   Loops          │   │             │  │            │  │            │    │
│   Import Repo    │   │ ◉ Active    │  │ ◉ Active   │  │ ○ Inactive │    │
│                  │   │ [  Start ▸ ]│  │ [  Start ▸ ]│  │ [  Start ▸ ]│   │
│ ─────────────── │   │ ↻ 📁 ⌥ 🗑   │  │ ↻ 📁 ⌥ 🗑   │  │ ↻ 📁 ⌥ 🗑   │    │
│                  │   └────────────┘  └────────────┘  └────────────┘    │
│                  │                                                      │
│ ⚙ Settings       │                                                      │
│ ─────────────── │                                                      │
│ ☀ Light │ ◓ Auto │ 🌙                                   v0.1.0  ⌘K     │
└─────────────────┴──────────────────────────────────────────────────────┘
   240px sidebar                                              flex-1
```

**Notes for Lovable:**
- Sidebar bg = `--surface` (one step below background), main bg = `--background`.
- Hairline border between them (`border-border`).
- Active nav item: subtle `--accent` fill + **2px violet left bar**.
- Brand mark = violet gradient square with white Command icon (keep existing).
- Footer: segmented theme toggle (left) + version + ⌘K hint (right).

### 3.2 App Shell — Dark Mode

```
┌─────────────────┬──────────────────────────────────────────────────────┐
│ ◆ LoopDeck      │  Dashboard                          [+ Import Repo] │
│ ENGINEERING     │  12 projects · 8 active                              │
│ COCKPIT         ├──────────────────────────────────────────────────────┤
│                 │                                                      │
│ ▸ Dashboard  ◀══│   ┌────────────┐  ┌────────────┐  ┌────────────┐    │
│   Activity       │   │ ▔▔▔▔▔▔▔▔▔▔▔ │  │ ▔▔▔▔▔▔▔▔▔▔▔ │  │ ▔▔▔▔▔▔▔▔▔▔▔ │    │
│   Agent Runner   │   │ L  loopdeck │  │ A  agentui │  │ F  foobar  │    │
│   Decisions      │   │             │  │            │  │            │    │
│   Loops          │   │ ◉ Active    │  │ ◉ Active   │  │ ○ Inactive │    │
│   Import Repo    │   │ [  Start ▸ ]│  │ [  Start ▸ ]│  │ [  Start ▸ ]│   │
│                  │   │ ↻ 📁 ⌥ 🗑   │  │ ↻ 📁 ⌥ 🗑   │  │ ↻ 📁 ⌥ 🗑   │    │
│ ─────────────── │   └────────────┘  └────────────┘  └────────────┘    │
│ ⚙ Settings       │                                                      │
│ ☀ │ ◓ Auto │ 🌙                                                  │
│                  │                                            v0.1.0 ⌘K │
└─────────────────┴──────────────────────────────────────────────────────┘
   darker surface, faint borders, no shadows — depth via elevation only
```

**Dark-mode-specific:**
- Cards are `--card` (slightly elevated over `--surface`).
- The accent gradient top-border on cards glows more vividly.
- No drop shadows anywhere — borders do the work.
- Active nav left-bar uses brighter violet.

---

### 3.3 ProjectCard — Anatomy (the most-touched component)

```
┌──────────────────────────────────────┐
│▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔│  ← gradient top accent (per-project hue)
│  ┌──┐  loopdeck                ↗    │  ← monogram (accent-tinted) + name + arrow
│  │ L│  Opened 2h ago                │
│  └──┘                                │
│                                      │
│  Local-first desktop app for         │  ← 2-line description, muted
│  structured project memory…          │
│                                      │
│  ⎇  Last commit · 3h ago             │  ← commit row (subtle bg pill)
│     feat: add streaming agent chat   │
│  ⏱  Folder modified · 1h ago         │  ← modified row
│  ↻  Current Loop · Refactor chat UI  │  ← current loop row
│                                      │
│  ● Active                            │  ← status badge, mt-auto
│                                      │
│  ┌──────────── Start ▸ ────────────┐ │  ← primary CTA, full width
│  └─────────────────────────────────┘ │
│  ──────────────────────────────────  │  ← divider
│   ↻      📁      ⌥      🗑            │  ← icon button row (ghost)
└──────────────────────────────────────┘
```

**Redesign notes:**
- Top accent gradient: **keep** — it's a signature. But soften: 60% opacity, fade at both ends.
- Commit/modified/loop rows: group into a single subtle `rounded-md bg-muted/30 p-2` block instead of three separate bordered pills. Less noise.
- Status badge: switch from circle+text to a proper **pill** (see Badge anatomy).
- Action row: ghost icon buttons, equal width, hover reveals labels (tooltip).
- Hover: card lifts 1px (`translateY(-1px)`) + border becomes `primary/40`.

---

### 3.4 Empty State — Dashboard (first run)

```
┌──────────────────────────────────────────────────────┐
│                                                      │
│                                                      │
│                   ╭────────╮                         │
│                   │  📂    │                         │
│                   ╰────────╯                         │
│                                                      │
│              No projects found                       │
│                                                      │
│         Scan a folder to discover repositories       │
│         and create project memory. LoopDeck          │
│         stores context inside each repository.       │
│                                                      │
│            ┌──────────────────────┐                  │
│            │   📁  Scan Folder    │                  │
│            └──────────────────────┘                  │
│                                                      │
│                                                      │
└──────────────────────────────────────────────────────┘
```

**Notes:**
- Icon: large (64px) but at **30% opacity** — ghostly, not loud.
- Generous vertical padding (`py-24`).
- Button: primary, centered, comfortable.
- Optional: a faint dotted-border drop zone hint around the icon on hover ("drag a folder here").

---

### 3.5 Project Detail — Overview Tab

```
┌─ ProjectDetail ─────────────────────────────────────────────────────┐
│  ←  loopdeck                                          ⌘            │  ← PageHeader: back arrow, name, path
│     /Users/suprie/Workspace/others/loopdeck                          │
├──────────────┬──────────────────────────────────────────────────────┤
│  loopdeck    │  Overview                                            │
│  /Users/...  │                                                      │
│              │  ┌────────────────────────────────────────────────┐  │
│  ▪ Overview  │  │ PATH                                            │  │
│    Decisions │  │ /Users/suprie/Workspace/others/loopdeck        │  │  ← mono, muted
│    Loops     │  │                                                 │  │
│    Agent     │  │ DESCRIPTION                          [✏] [↻]   │  │  ← edit/regenerate icons right-aligned
│              │  │ Local-first desktop app for structured          │  │
│              │  │ project memory — stored right inside your repo. │  │
│              │  │                                                 │  │
│              │  │ DETAILS                                         │  │
│              │  │ Status: Active   Created: Jun 22   Opened: 2h   │  │
│              │  │                                                 │  │
│              │  │ REPOSITORY ACTIVITY                             │  │
│              │  │ ⎇ Last commit: 3h ago — feat: streaming chat    │  │
│              │  │ ⏱ Last modified: 1h ago                          │  │
│              │  └────────────────────────────────────────────────┘  │
│              │                                                      │
│              │  [↻ Rescan] [📁 Finder] [⌥ Terminal] [🗑 Remove]     │  ← ghost buttons, Remove is destructive
└──────────────┴──────────────────────────────────────────────────────┘
   180px detail nav              max-w-2xl content, generous padding
```

**Notes:**
- The inner detail-nav mirrors the main sidebar styling (active = accent + left bar).
- Overview card uses section labels (`PATH`, `DESCRIPTION`) as `text-[10px] uppercase tracking-wider text-muted-foreground font-semibold`.
- Edit/regenerate icons: ghost `IconButton` size sm, top-right of the description block.
- Action row at the bottom: all ghost except Remove (destructive ghost).

---

### 3.6 Agent Chat — the signature screen

This is where LoopDeck should feel most premium. Model it on Linear's issue view + Claude's chat.

```
┌─ Agent · loopdeck ──────────────────────────────────────────────────┐
│  Agent                                               [＋ New chat]   │  ← header
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│       ╭──╮                                                           │
│       │👤│   What's the current loop for this project?                │  ← user bubble, right-aligned, primary bg
│       ╰──╯                                                           │
│                                                                      │
│  ╭──╮                                                                 │
│  │🤖│   ▾ Show thinking (1,247 chars)                                │  ← assistant bubble, card bg, left-aligned
│  ╰──╯                                                                │
│       › Read · src/components/detail/Chat.tsx                        │  ← tool calls, mono, muted
│       › Grep · pattern="useAppStore"                                 │
│                                                                      │
│       The current loop is "Refactor chat UI". Based on loops.md,     │  ← main answer
│       the next step is to extract the BlockList component…           │
│                                                                      │
│       4.2s · 1,204→856 tok · $0.0124                                 │  ← meta row, tiny, muted
│                                                                      │
│  ╭──────────────────────────────────────────────────────────────╮   │
│  │ ● The agent needs your approval                              │   │  ← approval card, primary-tinted border
│  │                                                               │   │
│  │ EDIT                                                          │   │
│  │ Edit · src/components/detail/Chat.tsx                         │   │
│  │                                                               │   │
│  │              [ 🛡 Deny ]    [ 🛡 Allow ]                      │   │
│  ╰──────────────────────────────────────────────────────────────╯   │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│ ┌────────────────────────────────────────────────────┐  ┌──┐        │  ← composer: textarea + send button
│ │ Send a follow-up message…                          │  │▸│        │
│ │                                                     │  │  │        │
│ └────────────────────────────────────────────────────┘  └──┘        │
│                                  Enter to send · Shift+Enter newline │
└──────────────────────────────────────────────────────────────────────┘
```

**Notes:**
- User bubble: `bg-primary text-primary-foreground`, right-aligned, max-w-[85%], `rounded-lg` but with a flat bottom-right corner (chat tail feel).
- Assistant bubble: `bg-card border border-border`, left-aligned. No tail.
- Tool-call rows: `text-[11px] font-mono text-muted-foreground`, indented under the bubble with a violet `›` glyph.
- Streaming cursor: 1.5px wide blinking violet bar after the last text block (keep existing).
- Approval/Question cards: `border-primary/30 bg-primary/5` — they should feel like gentle interruptions, not alarms.
- Composer: `bg-input border border-border rounded-lg`, grows to ~3 rows. Send button is a 36px primary square.
- Empty state: ghost `Bot` icon at 30% opacity + helpful copy referencing `.loopdeck/loops.md`.

---

### 3.7 Settings

```
┌─ Settings ──────────────────────────────────────────────────────────┐
│  Settings                                                           │
│  Configure your AI agent provider                                   │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ⚙ Agent Configuration                                               │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │                                                                │ │
│  │  Auth Token                                                    │ │
│  │  ┌──────────────────────────────────────────────────────────┐ │ │
│  │  │ ••••••••••••••••••                                        │ │ │
│  │  └──────────────────────────────────────────────────────────┘ │ │
│  │  Your API key. Stored locally in ~/.config/loopdeck/config.   │ │
│  │                                                                │ │
│  │  Base URL                                                      │ │
│  │  ┌──────────────────────────────────────────────────────────┐ │ │
│  │  │ https://api.anthropic.com                                 │ │ │
│  │  └──────────────────────────────────────────────────────────┘ │ │
│  │  Provider endpoint. Leave blank for default.                  │ │
│  │                                                                │ │
│  │  Model                      Effort Level                       │ │
│  │  ┌──────────────────────┐   ┌─────────────────────┐           │ │
│  │  │ claude-sonnet-4-6  ▾ │   │ High              ▾ │           │ │
│  │  └──────────────────────┘   └─────────────────────┘           │ │
│  │                                                                │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  [ 💾 Save Configuration ]      ✓ Configuration saved successfully   │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

**Notes:**
- Two-column layout for Model + Effort (they're short fields, don't waste vertical space).
- Inputs: `h-9 rounded-md bg-background border border-input focus:ring-2 ring-ring`.
- Save button: primary. Success state shows green check + message for 2.5s.
- Hint text: `text-[11px] text-muted-foreground mt-1.5`.

---

## 4. How to use this brief with Lovable

**Recommended sequence (one iteration each):**

1. **Foundation** — "Implement the design tokens and a `ThemeProvider` with light/dark/auto. Create the `src/components/ui/` primitives (Button, Card, Badge, IconButton, NavItem, ThemeToggle). Follow section 1 and 2 of the attached brief."

2. **App Shell + Dashboard** — "Redesign `AppShell` and `Dashboard` using the new primitives. Refer to mockup 3.1 (light) and 3.2 (dark). Add the theme toggle to the sidebar footer."

3. **ProjectCard** — "Redesign `ProjectCard` per mockup 3.3. Group the activity rows into one block, convert status to a pill, refine the action row."

4. **Project Detail** — "Redesign `ProjectDetail` overview per mockup 3.5, including the inner detail-nav."

5. **Agent Chat** — "Redesign `Chat.tsx` per mockup 3.6. This is the signature screen — spend the most effort here."

6. **Settings + Empty States** — "Finish with Settings (3.7) and the Dashboard empty state (3.4)."

**Paste rule:** For each iteration, paste the **relevant mockup ASCII block** plus the matching "Notes" bullets. Lovable follows ASCII layouts surprisingly well when paired with explicit token references like `bg-card`, `border-border`, `text-muted-foreground`.

---

## 5. Critical constraints (tell Lovable every time)

- **Do not touch IPC.** Never edit `src/lib/tauri.ts`, any `invoke()` call, or component props that wire to Tauri commands. Visual layer only.
- **Keep all functionality.** Every button, action, and prop must still work identically after the redesign.
- **Tailwind v4 conventions.** Use `bg-background`, `text-foreground`, `border-border`, `bg-primary`, etc. — the tokens are already mapped in `@theme inline`.
- **OKLCH, not HEX.** All new colors must be `oklch(...)` to match the existing system.
- **Inter + JetBrains Mono only.** Don't introduce new fonts.
- **Don't remove the `.dark` class strategy** (`@custom-variant dark (&:is(.dark *))`) — extend it.
