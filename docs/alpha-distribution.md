# LoopDeck — Private Alpha Distribution Contract

> **Status:** Alpha (Gate A). This is the authoritative contract for the private
> alpha build. It intentionally names **one** supported OS and makes **no**
> promises about anything else. The public V0.1 contract (Gate B) broadens this.

| | |
|---|---|
| **Supported OS** | **macOS on Apple Silicon (arm64)** |
| **Build/run OS of record** | macOS (matches the developer's machine and the single-OS CI in `.github/workflows/ci.yml`) |
| **Artifact** | Unsigned `.dmg` (Disk Image) → `LoopDeck.app` |
| **Code signing / notarization** | **None.** Deferred for recurring-cost reasons (see decision 2026-07-20). The install path below includes the Gatekeeper bypass this requires. |

Everything in this document applies **only** to the macOS/Apple-Silicon alpha
build. The tag-triggered `.github/workflows/build.yml` also happens to emit
Linux (`.deb`/`.AppImage`) and Windows (`.exe`/`.msi`) artifacts, but those are
**unsupported experiments toward Gate B**, not part of this contract — they are
not installation-tested, not signed, and not covered here.

---

## 1. The one supported OS

**macOS on Apple Silicon (arm64).**

- The alpha is built and run on macOS (the developer's platform, `darwin`).
- The release `.dmg` is an **arm64** build (`macos-latest` is Apple Silicon),
  so it runs natively on M-series Macs.
- **Intel (x86_64) Macs are not supported** in the alpha — no fat/universal
  binary is produced, and running the arm64 build under Rosetta is not tested.
- **No minimum macOS version is regression-tested for the alpha.** It is
  developed on the current macOS release; assume a recent version (macOS 13 /
  Ventura or newer). A specific floor will be pinned when Gate B adds an
  OS-matrix smoke test.

## 2. Distribution artifact

The alpha ships as an **unsigned `.dmg`** containing `LoopDeck.app`.

- **Build it locally:**
  ```bash
  npm ci
  npm run tauri build      # produces the .dmg + .app under src-tauri/target/release/bundle/
  ```
- **Or from CI:** tagging a release (`git tag v0.1.0`) triggers
  `.github/workflows/build.yml`, which builds the macOS `.dmg` and attaches it
  to the GitHub Release. (Signing is intentionally off in that workflow.)

Because the build is unsigned, macOS Gatekeeper will block first launch. The
next two sections deal with that.

## 3. Prerequisites

Before installing, the target Mac must have:

1. **macOS on Apple Silicon** (see §1).
2. **The `claude` CLI, installed and on `PATH`.** LoopDeck spawns `claude` as a
   subprocess per project; the agent runtime does not function without it.
   LoopDeck resolves `claude` to an **absolute, vetted path** at spawn (it
   skips non-absolute `PATH` entries to defeat cwd hijack — see decision
   2026-07-10).
   - ⚠️ **GUI-launch caveat (known alpha limitation):** apps launched from
     Finder/Spotlight/Dock receive a *minimal* `PATH` that typically omits
     Homebrew (`/opt/homebrew/bin`) and npm-global / version-manager bin
     directories. If LoopDeck can't find `claude` after a GUI launch, launch
     once from a terminal (`open -a LoopDeck`, after ensuring the terminal's
     `PATH` is exported) or ensure `claude` is in `/usr/local/bin`. A
     discover-common-install-dirs fallback is a tracked follow-up, not an
     alpha deliverable.
3. **`git` on `PATH` (advisory, not required).** Used only for project
   metadata (last commit, status, diff). If absent, projects simply show "no
   git info" — the app is otherwise fully functional. Resolved through the same
   absolute-path vetting as `claude`.
4. **An agent auth token, configured in-app.** On first launch, open
   **Settings** and paste your provider auth token. It is stored in a local
   **owner-only file** at `~/Library/Application Support/com.loopdeck.LoopDeck/agent_token`
   (`0600`, atomic-written) — never in the registry, and never in the macOS
   Keychain. (The Keychain path was dropped because an unsigned/un-notarized app
   triggers a password prompt on every access; see decision 2026-07-22.)

## 4. Installation

1. Obtain the `LoopDeck_*_aarch64.dmg` (local build output or the GitHub
   Release asset).
2. Double-click the `.dmg` to mount it, then drag **LoopDeck.app** into
   **/Applications**.
3. **Bypass Gatekeeper** (required because the build is unsigned). Do **one** of:
   - Terminal (fastest, recommended for the alpha):
     ```bash
     xattr -dr com.apple.quarantine /Applications/LoopDeck.app
     ```
   - Or in Finder: right-click `LoopDeck.app` → **Open** → confirm **Open** at
     the "unidentified developer" prompt. (The plain double-click path will
     only offer "Move to Trash" until this is done once.)
4. Launch from **/Applications** or Spotlight.
5. Complete **Settings**: paste the agent auth token (→ local secrets file),
   confirm base URL / model / effort.
6. Import your first repository from the **Import** view.

## 5. Upgrade / reinstall

LoopDeck is stateless as an application — the `.app` bundle carries no user
data, so upgrading is just replacing the bundle.

1. **Quit LoopDeck fully** (⌘Q from the app, or the menu → Quit; do not just
   close the window — a lingering process can hold the per-project session
   lock).
2. Replace `/Applications/LoopDeck.app` with the new build:
   - Re-mount the new `.dmg` and drag **LoopDeck.app** into **/Applications**,
     choosing **Replace** when macOS prompts.
3. If the new build is also unsigned, re-run the Gatekeeper strip from §4
   step 3 (`xattr -dr …`). (Repeating it is harmless.)
4. Launch. All project state, the registry, transcripts, and the local
   secrets-file token carry over unchanged (see §7).

> **Upgrading past the Keychain removal (build with decision 2026-07-22 or
> later):** if you had a token in the macOS Keychain from an earlier build, it
> is **not** migrated automatically — the Keychain is no longer read. Re-enter
> your token once in **Settings** on first launch of the new build; it will be
> stored in the new `agent_token` file and persist from there.

**Reinstall (same version, clean-ish):** identical to upgrade — quit, replace
the `.app`, relaunch. Reinstalling does **not** touch `.loopdeck/` directories,
the global registry, or the secrets file, so it is safe as a "try again" step if
a build misbehaves.

> **No migration guarantees across alpha versions.** Alpha builds may change
> the on-disk shapes of `.loopdeck/project.yaml`, the registry, or transcripts
> without migration code. Assume forward-compatibility is best-effort between
> alpha versions; keep a copy of any build you rely on for rollback (§6).

## 6. Rollback

1. **Quit LoopDeck fully** (⌘Q).
2. Replace `/Applications/LoopDeck.app` with the **previous build** you kept
   (drag the old `.app` back from the old `.dmg`, choosing **Replace**).
3. Re-strip quarantine if needed (§4 step 3).
4. Launch.

**If the newer alpha corrupted the global registry** (refuses to launch, or
shows an empty project list): LoopDeck never silently overwrites a malformed
registry — on a malformed primary it logs a structured error and exits rather
than wiping your data (decision 2026-07-19). Recover by hand:

- The last-known-good copy lives beside it as **`config.yaml.bak`** (same
  directory — see §7). Quit LoopDeck, then:
  ```bash
  cd "$HOME/Library/Application Support/com.loopdeck.LoopDeck"
  cp config.yaml.bak config.yaml     # restore last good registry
  ```
- Or roll back the `.app` to the prior alpha (which wrote the `.bak`) and
  relaunch — it will read the registry it last wrote successfully.

Per-repo `.loopdeck/` data is never touched by an app rollback; the worst case
is a transcript `active.jsonl` whose last line is an orphaned `user` turn from
an interrupted run — LoopDeck reconciles that into a truthful "interrupted"
marker on next launch (decision 2026-07-19), so no manual cleanup is needed.

## 7. Where your data lives

Knowing these paths is what makes backup, rollback, and manual recovery
possible. LoopDeck is local-first and offline-first — there is **no cloud
copy** of any of this.

| What | Where (macOS) | Notes |
|---|---|---|
| Global registry (project list, settings) | `~/Library/Application Support/com.loopdeck.LoopDeck/config.yaml` | Atomic-written, `0600`. **Not** `~/.config/loopdeck/` — that path appears in code comments but is the Linux/fallback path; on macOS `directories::ProjectDirs::config_dir()` resolves under `~/Library/Application Support`. |
| Registry backup | `~/Library/Application Support/com.loopdeck.LoopDeck/config.yaml.bak` | Last-known-good copy, written before every registry overwrite. |
| Per-repo project memory | `<repo>/.loopdeck/project.yaml` | Travels with the repo. Atomic-written. |
| Per-repo decisions / loops | `<repo>/.loopdeck/decisions.md`, `loops.md` | Markdown, agent-written. |
| Conversation transcripts | `<repo>/.loopdeck/sessions/active.jsonl` (+ `archive-*.jsonl`) | Append-only, one JSON object per line; `active` is rotated to a timestamped archive on session reset. |
| Agent auth token | `~/Library/Application Support/com.loopdeck.LoopDeck/agent_token` | Owner-only (`0600`), atomic-written. Separate from the registry. Use **Settings → Clear token** to revoke. |
| Diagnostic logs | `~/Library/Logs/LoopDeck/loopdeck.log.YYYY-MM-DD` | See §8. |

**To fully back up** your LoopDeck state: back up each imported repo (the
`.loopdeck/` dirs), plus the global registry directory above (which includes
`agent_token`).

**To fully uninstall:** remove `/Applications/LoopDeck.app`, delete the global
registry directory above (which removes `agent_token`), and remove `.loopdeck/`
from each repo. (If you used an **older** build that stored the token in the
macOS Keychain, that Keychain item is left behind by this version — remove it
manually via **Keychain Access** if you wish.)

## 8. Diagnostic logs

LoopDeck writes one rolling log file per day:

- **Location:** `~/Library/Logs/LoopDeck/loopdeck.log.YYYY-MM-DD`
- **Override the directory:** set `LOOPDECK_LOG_DIR=/some/path` before launch.
- **Verbosity:** set `RUST_LOG` (default is `loopdeck=info,warn`). To capture
  the raw NDJSON wire traffic to/from the `claude` process while debugging a
  stalled turn or control-protocol issue:
  ```bash
  RUST_LOG=loopdeck=debug open -a LoopDeck
  ```
- **Retention:** logs are rolled daily and **bounded to the last 14 daily
  files** (`logging::MAX_LOG_FILES`). Older files are pruned automatically —
  at startup and on each daily rollover — so the log directory cannot grow
  without bound. The auth token is **never** logged (the `AgentConfig` `Debug`
  impl redacts it, and a regression test pins that invariant).
- **Viewing:** the easiest path is **Settings → Diagnostics → "Open logs
  folder"**, which reveals the directory in Finder and shows the current file
  count, total size, and the retention cap. You can also open the file
  directly, or use **Console.app** / `log show` (the directory is the standard
  macOS per-app log dir).

The log directory is resolved in-process (`logging::log_dir()`) and surfaced
over IPC by the `get_log_info` / `reveal_log_dir` commands that back the
Diagnostics panel.

---

## Known limitations (alpha)

- **Unsigned build** → requires the §4 Gatekeeper bypass on every fresh
  install/reinstall.
- **GUI-launch minimal `PATH`** → may fail to find `claude`/`git` installed
  under Homebrew or npm-global until a common-bin-dir fallback lands (§3).
- **Apple Silicon only** → no Intel build (§1).
- **No cross-version migration guarantees** within the alpha series (§5).

## Out of scope for this contract

The following are explicitly **not** promised by the private alpha and are
tracked as separate Gate B / backlog work:

- Linux or Windows support (despite `build.yml` emitting artifacts for them).
- Signed / notarized builds.
- An auto-update mechanism (upgrades are manual `.app` replacement for the
  alpha — §5).
- A formal minimum-macOS-version guarantee.
- User-facing "reveal logs" UI wiring (the path is stable and documented in §8).
