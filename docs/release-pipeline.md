# LoopDeck — Release Artifact Pipeline

> **Status:** Gate B (Public V0.1 foundation). This document defines **how a
> LoopDeck release artifact is produced and verified**. It is the companion to
> [`alpha-distribution.md`](./alpha-distribution.md), which defines **how a user
> installs, upgrades, and rolls back** an artifact once it exists.
>
> - `alpha-distribution.md` → the **install/upgrade/rollback contract** (what the
>   user does with an artifact).
> - `release-pipeline.md` → the **build/release contract** (how an artifact is
>   made and smoke-tested before a user ever sees it).

---

## 1. Scope and the one supported artifact

Consistent with the alpha distribution contract, **macOS on Apple Silicon
(arm64)** is the one *supported* release target. The pipeline produces one
*supported artifact* and two *experimental* artifacts:

| Artifact | OS | Status | Installer format |
|---|---|---|---|
| **`LoopDeck_<v>_aarch64.dmg`** | macOS / Apple Silicon (arm64) | ✅ **Supported** — smoke-tested | `.dmg` → `LoopDeck.app` |
| `LoopDeck_<v>_amd64.deb` / `.AppImage` | Linux (x86_64) | 🧪 Experimental | `.deb`, `.AppImage` |
| `LoopDeck_<v>_x64-setup.exe` / `.msi` | Windows (x86_64) | 🧪 Experimental | `.exe` (NSIS), `.msi` |

- **Supported** = built, installation-smoke-tested (see §6), documented install
  path, and covered by `alpha-distribution.md`.
- **Experimental** = the pipeline *compiles and bundles* them so cross-OS
  regressions are visible, but they are **not installation-tested, not signed,
  and not covered by the install contract.** They exist to de-risk Gate B's
  eventual broadening; they are not a promise to users.

> **Code signing / notarization: OFF.** Deferred for recurring-cost reasons
> (decision 2026-07-20). The supported artifact is an **unsigned** `.dmg`; the
> install path includes the Gatekeeper bypass this requires (see
> `alpha-distribution.md` §4). Re-enabling signing is documented in §7.

## 2. Where the version lives

LoopDeck's version is **declared in three places that must agree**, and the
release tag must match all three:

| File | Field |
|---|---|
| `package.json` | `"version"` |
| `src-tauri/tauri.conf.json` | `"version"` (under `bundle`/root) |
| `src-tauri/Cargo.toml` | `version` |

- The pipeline does **not** bump the version for you — bumping is a deliberate,
  human-authored commit (decision: a release is a reviewable act, not an
  auto-increment).
- **Tag format:** `v<semver>`, e.g. `v0.1.0`, `v0.1.1` (matches the existing
  `v0.1.1` tag and `build.yml`'s `on.push.tags: ['v*']` trigger).
- The tag name becomes the GitHub Release name and the asset filename suffix.

## 3. The two build paths

There are two ways to produce the supported artifact. They run the **same**
commands, so a local build is a faithful preview of the CI build.

### 3a. Local build (developer machine)

```bash
npm ci                     # frontend deps (idempotent)
npm run tauri build        # = tsc → vite build → cargo build --release → bundle
```

Output lands under:

```
src-tauri/target/release/bundle/
├── macos/
│   └── LoopDeck.app                          # the app bundle
└── dmg/
    └── LoopDeck_<v>_aarch64.dmg              # the distributable disk image
```

### 3b. CI build (`.github/workflows/build.yml`)

Triggered by **pushing a `v*` tag** (or `workflow_dispatch` from the Actions
tab). It builds the artifact matrix (§1) via `tauri-apps/tauri-action@v0`,
which attaches each artifact to a **GitHub Release** named after the tag.

- `releaseDraft: false`, `prerelease: false` — tagging publishes.
- `permissions: contents: write` — required to upload release assets.
- Signing env vars are **omitted** (not set to empty secrets), so the bundler
  emits an unsigned `.dmg` (see the long comment in `build.yml`).

## 4. Pipeline stages

```
┌─────────────────────────────────────────────────────────────────┐
│  TRIGGER                                                        │
│   git tag v<x.y.z> && git push origin v<x.y.z>                  │
│   (version in package.json / tauri.conf.json / Cargo.toml =     │
│    x.y.z)                                                       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  STAGE 1 — Frontend build                                       │
│   npm ci  →  tsc (type-check)  →  vite build  →  ../dist         │
│   (tauri.conf.json beforeBuildCommand)                          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  STAGE 2 — Rust release build                                   │
│   cargo build --release  (src-tauri/)                           │
│   Produces the native binary the webview hosts.                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  STAGE 3 — Bundle                                               │
│   tauri-bundler → LoopDeck.app (+ .dmg on macOS; .deb/.AppImage │
│   on Linux; .exe/.msi on Windows). Signing OFF.                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  STAGE 4 — Publish (CI only)                                    │
│   tauri-action attaches each artifact to the GitHub Release     │
│   named v<x.y.z>.                                              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  STAGE 5 — Smoke test  (see §6)                                 │
│   scripts/smoke-test-release.sh — install/upgrade/reinstall/    │
│   rollback state invariants against the produced artifact.      │
│   Run locally before tagging; the manual GUI smoke (§6b) is the │
│   human sign-off before announcing a release.                   │
└─────────────────────────────────────────────────────────────────┘
```

## 5. Cutting a release (checklist)

1. **Confirm `main` is green.** The Gate A CI (`.github/workflows/ci.yml`)
   runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`,
   `npm ci`, `npm run build` on macOS. Do not tag over a red CI.
2. **Bump the version** in all three files (§2) in one commit.
3. **Build locally** (`npm run tauri build`) and **run the smoke test**
   (`scripts/smoke-test-release.sh` against the local build — §6). Do not tag
   if the smoke test fails.
4. **Do the manual GUI smoke** (§6b) on the local build.
5. **Tag and push:** `git tag v<x.y.z> && git push origin v<x.y.z>`.
6. `build.yml` builds the matrix and publishes the GitHub Release.
7. Download the CI-produced `.dmg` and re-run the smoke test against **that**
   asset (not your local build) to confirm CI parity. Edit the release notes
   with a summary + the "where your data lives" pointer from
   `alpha-distribution.md` §7.

## 6. Smoke test

The smoke test verifies the **behavioral invariants** the install contract
promises, not just that a file exists. It lives at
[`../scripts/smoke-test-release.sh`](../scripts/smoke-test-release.sh).

### 6a. Automated — `scripts/smoke-test-release.sh`

A hermetic, CI-runnable shell test. It exercises, against an isolated temp
"Applications" directory and an isolated config directory, the four lifecycle
operations the contract describes:

| Lifecycle op | What the script asserts |
|---|---|
| **Install** | An `.app` placed in the apps dir + quarantine stripped is the terminal installed state; nothing is written to the config dir by the act of installing. |
| **Upgrade** (replace `.app`) | Replacing the `.app` bundle with a "new" build leaves the config dir, registry, `agent_token`, and per-repo `.loopdeck/` **byte-for-byte unchanged** — the core "state lives outside the bundle" invariant. |
| **Reinstall** (replace with same build) | Identical to upgrade; reinstall is a safe "try again" and touches no user data. |
| **Rollback** (restore prior `.app` + registry `.bak`) | The documented `cp config.yaml.bak config.yaml` recovery restores the last-known-good registry; `.loopdeck/` data is untouched by any rollback. |

**Two modes:**

- **Hermetic (default):** uses a synthetic `.app` skeleton. Runs anywhere, no
  build required. Validates the *file-system invariants* — which is exactly
  what "state lives outside the bundle" reduces to. Use this in CI / as a fast
  pre-tag gate.
- **Real artifact (`--app <path>` / `--dmg <path>`):** additionally mounts a
  real `.dmg`, copies the real `LoopDeck.app`, and asserts the bundle's
  internal structure (`Contents/MacOS/LoopDeck` binary present,
  `Contents/Info.plist` present). Use this against a freshly built
  `src-tauri/target/release/bundle/...` output before tagging.

Run it:

```bash
# hermetic — fast, no build needed
scripts/smoke-test-release.sh

# against a real local build
scripts/smoke-test-release.sh \
  --app src-tauri/target/release/bundle/macos/LoopDeck.app

# against a real .dmg (mounts, copies, unmounts)
scripts/smoke-test-release.sh \
  --dmg src-tauri/target/release/bundle/dmg/LoopDeck_*_aarch64.dmg
```

The script **never touches the real** `~/Library/Application Support/...`,
`/Applications`, or any real `.loopdeck/` — it works entirely inside a temp
dir it creates and cleans up. It exits non-zero on the first failed assertion.

### 6b. Manual — GUI sign-off (the human part)

The automated smoke verifies the on-disk invariants; it cannot launch the GUI
on a headless runner. Before announcing a release, do this once on a real Mac:

1. Install per `alpha-distribution.md` §4 (drag to `/Applications`, strip
   quarantine, launch).
2. **Settings** → paste an agent auth token; confirm it persists at
   `~/Library/Application Support/com.loopdeck.LoopDeck/agent_token`.
3. Import one repo; start a turn; approve/deny one permission; confirm the
   transcript lands in `<repo>/.loopdeck/sessions/active.jsonl`.
4. Quit (⌘Q). **Upgrade**: replace `/Applications/LoopDeck.app` with the next
   build, relaunch. Confirm the imported project, the token, and the transcript
   are all still present (state survived the bundle replacement).
5. **Rollback**: replace with the prior build + the §6 registry `.bak` path;
   confirm the project list restores.

Steps 4–5 are the human-verified version of what the automated smoke checks at
the file level.

## 7. Re-enabling signing / notarization (deferred)

Signing is intentionally off (decision 2026-07-20: the recurring Apple
Developer Program cost is not justified for an unsigned private alpha). When
distribution reach justifies it:

1. Acquire an Apple Developer ID Application certificate + notarization
   credentials.
2. Add the six repo secrets documented in the long comment in `build.yml`
   (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
   `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) — all six are required
   together.
3. Re-add the matching `env:` lines under the build step. `tauri-bundler` will
   then sign + notarize, and the Gatekeeper bypass in the install contract can
   be dropped.
4. At that point, revisit storing the agent auth token back in the macOS
   Keychain (decision 2026-07-22: the Keychain was dropped only because an
   unsigned build prompts on every access).

## 8. Out of scope

These are **not** part of this pipeline and are tracked elsewhere:

- **Auto-update mechanism.** Upgrades are manual `.app` replacement
  (`alpha-distribution.md` §5). An auto-updater is a separate future item.
- **Cross-version data migration.** No migration code runs between alpha/V0.1
  builds; forward-compatibility is best-effort (`alpha-distribution.md` §5).
- **Linux / Windows install contracts.** Those artifacts compile in the
  pipeline but are not installation-tested (§1).
- **A published CHANGELOG.** Release notes are authored by hand in step 7 of
  the cut checklist; automated changelog generation is a P5 backlog item.
- **SBOM / license / dependency-audit gating in CI.** P3 backlog item.
