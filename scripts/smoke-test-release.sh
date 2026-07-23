#!/usr/bin/env bash
#
# smoke-test-release.sh — verify the LoopDeck install/upgrade/reinstall/rollback
# contract at the file-system level.
#
# This is the automated half of the release smoke test defined in
# docs/release-pipeline.md §6. Its job is narrow and deliberate: prove the
# *behavioral invariants* the install contract (docs/alpha-distribution.md)
# promises — not just that a file exists.
#
# The single most important invariant — "state lives outside the .app bundle"
# (alpha-distribution.md §5/§7) — reduces to a file-system check: none of the
# four lifecycle operations (install, upgrade, reinstall, rollback) may modify
# the config dir, the global registry, the agent_token file, or any per-repo
# .loopdeck/ directory. This script asserts exactly that, byte-for-byte.
#
# It does NOT launch the GUI (it cannot on a headless runner). The manual GUI
# sign-off in docs/release-pipeline.md §6b is the human counterpart to this
# script and remains a required step before announcing a release.
#
# USAGE
#   # Hermetic (default) — synthetic .app skeleton, no build required, portable.
#   # Runs anywhere; validates the file-system invariants. Fast pre-tag gate.
#   scripts/smoke-test-release.sh
#
#   # Real artifact — copy a built LoopDeck.app and also assert its bundle
#   # structure (Contents/MacOS/LoopDeck + Contents/Info.plist).
#   scripts/smoke-test-release.sh \
#     --app src-tauri/target/release/bundle/macos/LoopDeck.app
#
#   # Real .dmg — mount, copy LoopDeck.app out, unmount, then assert structure.
#   scripts/smoke-test-release.sh \
#     --dmg src-tauri/target/release/bundle/dmg/LoopDeck_*_aarch64.dmg
#
# SAFETY
#   This script NEVER touches the real ~/Library/Application Support/...,
#   /Applications, or any real .loopdeck/. Everything happens inside a temp
#   directory it creates and removes on exit. It exits non-zero on the first
#   failed assertion.
#
# See: docs/release-pipeline.md (build/release contract),
#      docs/alpha-distribution.md  (install/upgrade/rollback contract).

set -uo pipefail

# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------

PASS=0
FAILED=0

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; PASS=$((PASS + 1)); }
fail() { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; FAILED=$((FAILED + 1)); }

# Hard fail: stop the whole run on the first broken invariant. The contract is
# fail-stop — a release that fails any invariant is not shippable, so there is
# no value in collecting further failures past the first.
die() { printf '\n\033[31mFATAL\033[0m: %s\n' "$1" >&2; exit 1; }

assert_eq() { # <actual> <expected> <label>
  if [ "$1" = "$2" ]; then ok "$3"; else fail "$3"; fail "    expected [$2]"; fail "    got      [$1]"; exit 1; fi
}

assert_neq() { # <a> <b> <label>
  if [ "$1" != "$2" ]; then ok "$3"; else fail "$3"; fail "    expected [$1] != [$2]"; exit 1; fi
}

uname_s() { uname -s 2>/dev/null || echo unknown; }

# macOS uses `stat -f '%Lp'`; Linux uses `stat -c '%a'`.
mode_of() { # <file> -> octal mode, e.g. "600"
  if [ "$(uname_s)" = "Darwin" ]; then stat -f '%Lp' "$1" 2>/dev/null
  else stat -c '%a' "$1" 2>/dev/null; fi
}

# Strip the Gatekeeper quarantine attribute. No-op off macOS — the supported
# artifact is macOS arm64, but the hermetic mode should still run on a Linux CI
# runner for fast feedback, so we degrade gracefully.
strip_quarantine() { # <path>
  if [ "$(uname_s)" = "Darwin" ]; then
    xattr -dr com.apple.quarantine "$1" 2>/dev/null || true
  fi
}

# Digest one or more paths (files or dirs) into a single SHA. Captures relative
# path names + file content so both structure and bytes are covered — a renamed
# or added file changes the digest even if total bytes are unchanged. This is
# the "byte-for-byte unchanged" check the contract leans on.
digest() { # <path...> -> sha
  local p
  {
    for p in "$@"; do
      if [ -d "$p" ]; then
        ( cd "$p" && find . -type f 2>/dev/null | sort | while IFS= read -r f; do
            printf '%s\t' "$f"; shasum "$f" | awk '{print $1}'
          done )
      elif [ -f "$p" ]; then
        printf '%s\t' "$(basename "$p")"; shasum "$p" | awk '{print $1}'
      fi
    done
  } | shasum | awk '{print $1}'
}

# ----------------------------------------------------------------------------
# Temp workspace setup + cleanup
# ----------------------------------------------------------------------------

WORK="$(mktemp -d -t loopdeck-smoke)" || die "could not create temp dir"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

APPS="$WORK/Apps"                                  # fake /Applications
CFGDIR="$WORK/config/com.loopdeck.LoopDeck"        # fake config dir (real name)
REPO="$WORK/repo"                                  # fake imported repo
BUILDS="$WORK/builds"                              # source .app skeletons
mkdir -p "$APPS" "$CFGDIR" "$REPO/.loopdeck/sessions" "$BUILDS"

# Populate the fake config dir with a realistic, known-good registry shape.
# config.yaml and config.yaml.bak hold the SAME good registry in steady state
# (the .bak is written before every overwrite — see config.rs backup_path).
GOOD_REGISTRY="version: 1
projects:
  - path: $REPO
    name: smoke-repo
    added_at: 2026-07-23T00:00:00Z
settings:
  base_url: https://api.example.com
  model: claude-sonnet-5
  effort: high
"
printf '%s\n' "$GOOD_REGISTRY" > "$CFGDIR/config.yaml"
printf '%s\n' "$GOOD_REGISTRY" > "$CFGDIR/config.yaml.bak"

# agent_token: owner-only (0600), atomic-written, separate from the registry.
printf 'sk-smoke-secret-token\n' > "$CFGDIR/agent_token"
chmod 600 "$CFGDIR/agent_token"

# Per-repo .loopdeck/ memory that must survive every lifecycle op untouched.
printf 'name: smoke-repo\ndescription: hermetic smoke repo\n' > "$REPO/.loopdeck/project.yaml"
printf '# Decisions\n\n## 2026-07-23 — smoke\n- Status: accepted\n' > "$REPO/.loopdeck/decisions.md"
printf '# Loops\n\n## Current\n- Status: in_progress\n' > "$REPO/.loopdeck/loops.md"
# A transcript with a final user turn (simulating an interrupted run) — the
# contract says LoopDeck reconciles this on next launch; here we only assert it
# is never mutated by an install/upgrade/rollback.
printf '{"type":"user","text":"hi"}\n' > "$REPO/.loopdeck/sessions/active.jsonl"

# Files whose content/mode must be byte-for-byte invariant across EVERY op.
INVARIANT_PATHS=(
  "$CFGDIR/agent_token"
  "$CFGDIR/config.yaml.bak"
  "$REPO/.loopdeck"
)
REGISTRY_PRIMARY="$CFGDIR/config.yaml"

# ----------------------------------------------------------------------------
# .app source construction (hermetic skeleton vs. real artifact)
# ----------------------------------------------------------------------------

APP_REAL=""      # set by --app/--dmg; empty => hermetic skeleton mode
DMG_REAL=""

# Build a minimal-but-valid .app skeleton at <dest> tagged with <label>.
# We never launch it; it only needs the bundle structure + a distinct version
# so an "upgrade" is observably a different bundle.
make_skeleton() { # <dest_app_path> <version_label>
  local app="$1" label="$2"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>LoopDeck</string>
  <key>CFBundleIdentifier</key>
  <string>com.loopdeck.app</string>
  <key>CFBundleShortVersionString</key>
  <string>${label}</string>
  <key>CFBundleVersion</key>
  <string>${label}</string>
</dict>
</plist>
EOF
  printf '#!/bin/sh\n# LoopDeck stub binary (smoke skeleton) %s\nexit 0\n' "$label" \
    > "$app/Contents/MacOS/LoopDeck"
  chmod +x "$app/Contents/MacOS/LoopDeck"
}

# Assert the bundle internal structure a real LoopDeck.app must have.
assert_bundle_structure() { # <app_path>
  local app="$1"
  [ -f "$app/Contents/Info.plist" ] || die "missing $app/Contents/Info.plist"
  [ -f "$app/Contents/MacOS/LoopDeck" ] || die "missing $app/Contents/MacOS/LoopDeck (main binary)"
  [ -x "$app/Contents/MacOS/LoopDeck" ] || die "LoopDeck binary not executable"
  ok "bundle structure valid: $app"
}

# Install/copy a source .app into the fake Applications dir (drag-to-Apps +
# Gatekeeper strip). This is the one operation "install" performs.
install_app() { # <src_app>
  local src="$1"
  [ -d "$src" ] || die "source app not found: $src"
  cp -R "$src" "$APPS/LoopDeck.app"
  strip_quarantine "$APPS/LoopDeck.app"
}

# Replace the installed .app with a (possibly different) source build. This is
# the upgrade / reinstall / rollback operation: quit (we don't model the
# process), remove the old bundle, drag the new one in, re-strip quarantine.
replace_app() { # <src_app>
  local src="$1"
  [ -d "$src" ] || die "source app not found: $src"
  rm -rf "$APPS/LoopDeck.app"
  cp -R "$src" "$APPS/LoopDeck.app"
  strip_quarantine "$APPS/LoopDeck.app"
}

# ----------------------------------------------------------------------------
# Argument parsing
# ----------------------------------------------------------------------------

usage() {
  sed -n '2,/^set -uo pipefail$/p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --app) APP_REAL="$2"; shift 2;;
    --dmg) DMG_REAL="$2"; shift 2;;
    -h|--help) usage 0;;
    *) echo "unknown arg: $1" >&2; usage 1;;
  esac
done

if [ -n "$APP_REAL" ] && [ -n "$DMG_REAL" ]; then
  die "--app and --dmg are mutually exclusive"
fi

# Resolve the real .app source, possibly by mounting a .dmg first.
REAL_APP_SRC=""
if [ -n "$DMG_REAL" ]; then
  [ "$(uname_s)" = "Darwin" ] || die "--dmg requires macOS (hdiutil)"
  command -v hdiutil >/dev/null || die "hdiutil not found"
  [ -f "$DMG_REAL" ] || die "dmg not found: $DMG_REAL"
  REAL_APP_SRC="$BUILDS/LoopDeck.app"
  mnt="$WORK/mnt"
  mkdir -p "$mnt"
  printf 'mounting %s ...\n' "$DMG_REAL"
  hdiutil attach -nobrowse -mountpoint "$mnt" "$DMG_REAL" >/dev/null \
    || die "hdiutil attach failed for $DMG_REAL"
  [ -d "$mnt/LoopDeck.app" ] || { hdiutil detach "$mnt" -quiet; die "no LoopDeck.app in dmg"; }
  cp -R "$mnt/LoopDeck.app" "$REAL_APP_SRC"
  hdiutil detach "$mnt" -quiet || true
elif [ -n "$APP_REAL" ]; then
  [ -d "$APP_REAL" ] || die "app not found: $APP_REAL"
  REAL_APP_SRC="$BUILDS/LoopDeck.app"
  cp -R "$APP_REAL" "$REAL_APP_SRC"
fi

# Decide the two "builds" used across the lifecycle. In hermetic mode we have
# distinct v1/v2 skeletons so an upgrade is observably a different bundle. In
# real-artifact mode we only have one build, so "upgrade"/"rollback" reuse it —
# the invariant under test (replacing the bundle never touches user data) is
# identical regardless of whether the replacement is a different build.
if [ -n "$REAL_APP_SRC" ]; then
  APP_PRIOR="$REAL_APP_SRC"     # the one real build
  APP_NEW="$REAL_APP_SRC"
  MODE="real-artifact"
else
  make_skeleton "$BUILDS/LoopDeck-v1.app" "0.1.0"
  make_skeleton "$BUILDS/LoopDeck-v2.app" "0.1.1"
  APP_PRIOR="$BUILDS/LoopDeck-v1.app"
  APP_NEW="$BUILDS/LoopDeck-v2.app"
  MODE="hermetic"
fi

printf '\n=== LoopDeck release smoke test [%s] ===\n' "$MODE"
printf 'workspace: %s\n\n' "$WORK"

# Baseline invariants captured before any operation.
BASE_INVARIANT="$(digest "${INVARIANT_PATHS[@]}")"
BASE_REGISTRY="$(digest "$REGISTRY_PRIMARY")"
BASE_TOKEN_MODE="$(mode_of "$CFGDIR/agent_token")"

# Sanity: the token must be owner-only from the start.
assert_eq "$BASE_TOKEN_MODE" "600" "agent_token is owner-only (0600) at baseline"

if [ "$MODE" = "real-artifact" ]; then
  assert_bundle_structure "$REAL_APP_SRC"
fi

# ----------------------------------------------------------------------------
# Lifecycle op 1 — INSTALL
# ----------------------------------------------------------------------------
printf '\n[1/4] install\n'
install_app "$APP_PRIOR"
[ -d "$APPS/LoopDeck.app" ] || die "install did not place LoopDeck.app"
ok "LoopDeck.app present in Applications dir"
[ "$(digest "${INVARIANT_PATHS[@]}")" = "$BASE_INVARIANT" ] \
  || die "install mutated user data (invariant paths changed)"
[ "$(digest "$REGISTRY_PRIMARY")" = "$BASE_REGISTRY" ] \
  || die "install mutated the registry"
ok "install left config dir, registry, agent_token, and .loopdeck/ untouched"

# ----------------------------------------------------------------------------
# Lifecycle op 2 — UPGRADE (replace .app with a newer build)
# ----------------------------------------------------------------------------
printf '\n[2/4] upgrade\n'
replace_app "$APP_NEW"
[ -d "$APPS/LoopDeck.app" ] || die "upgrade removed LoopDeck.app"
ok "new LoopDeck.app in place after upgrade"
[ "$(digest "${INVARIANT_PATHS[@]}")" = "$BASE_INVARIANT" ] \
  || die "upgrade mutated user data"
[ "$(digest "$REGISTRY_PRIMARY")" = "$BASE_REGISTRY" ] \
  || die "upgrade mutated the registry"
ok "upgrade left config dir, registry, agent_token, and .loopdeck/ byte-for-byte unchanged"

# In hermetic mode, additionally confirm the bundle actually changed.
if [ "$MODE" = "hermetic" ]; then
  NEW_VER="$(grep -A1 CFBundleShortVersionString "$APPS/LoopDeck.app/Contents/Info.plist" \
    | grep string | sed -E 's/.*<string>(.*)<\/string>.*/\1/')"
  assert_eq "$NEW_VER" "0.1.1" "upgrade installed the newer bundle (CFBundleShortVersionString)"
fi

# ----------------------------------------------------------------------------
# Lifecycle op 3 — REINSTALL (replace .app with the same build)
# ----------------------------------------------------------------------------
printf '\n[3/4] reinstall\n'
replace_app "$APP_NEW"
[ "$(digest "${INVARIANT_PATHS[@]}")" = "$BASE_INVARIANT" ] \
  || die "reinstall mutated user data"
[ "$(digest "$REGISTRY_PRIMARY")" = "$BASE_REGISTRY" ] \
  || die "reinstall mutated the registry"
ok "reinstall (same build) is a safe no-op on user data"

# ----------------------------------------------------------------------------
# Lifecycle op 4 — ROLLBACK (restore prior .app + registry .bak recovery)
# ----------------------------------------------------------------------------
printf '\n[4/4] rollback\n'

# 4a. Restore the prior .app bundle. User data stays untouched.
replace_app "$APP_PRIOR"
[ "$(digest "${INVARIANT_PATHS[@]}")" = "$BASE_INVARIANT" ] \
  || die "rollback (.app restore) mutated user data"
ok "restoring prior .app left agent_token, config.yaml.bak, and .loopdeck/ unchanged"

# 4b. The documented registry recovery (alpha-distribution.md §6): if a newer
# alpha corrupted the registry, `cp config.yaml.bak config.yaml` restores the
# last-known-good copy. Simulate corruption, then run the documented recovery,
# and assert it restores the original good registry.
printf 'CORRUPTED-GARBAGE-FROM-A-BAD-ALPHA\n' > "$REGISTRY_PRIMARY"
assert_neq "$(digest "$REGISTRY_PRIMARY")" "$BASE_REGISTRY" \
  "registry primary is corrupted before recovery"
# The exact recovery command a user would run, per the contract:
cp "$CFGDIR/config.yaml.bak" "$REGISTRY_PRIMARY"
assert_eq "$(digest "$REGISTRY_PRIMARY")" "$BASE_REGISTRY" \
  "cp config.yaml.bak config.yaml restored the last-known-good registry"
# The recovery touched only config.yaml; re-assert the full invariant set
# (agent_token + config.yaml.bak + .loopdeck/) is still at baseline.
assert_eq "$(digest "${INVARIANT_PATHS[@]}")" "$BASE_INVARIANT" \
  ".loopdeck/, agent_token, and config.yaml.bak untouched by rollback + recovery"

# ----------------------------------------------------------------------------
# Summary
# ----------------------------------------------------------------------------
printf '\n=== %s: %d assertion(s) passed ===\n' "$MODE" "$PASS"
printf 'workspace cleaned up: %s\n' "$WORK"

if [ "$MODE" = "hermetic" ]; then
  printf '\nNOTE: hermetic mode validated file-system invariants only.\n'
  printf 'Before tagging, also run against a real build:\n'
  printf '  scripts/smoke-test-release.sh --app src-tauri/target/release/bundle/macos/LoopDeck.app\n'
  printf 'and complete the manual GUI sign-off (docs/release-pipeline.md §6b).\n'
fi

printf '\n\033[32mSMOKE TEST PASSED\033[0m\n'
exit 0
