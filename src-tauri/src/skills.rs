use crate::config::AgentHarness;
use crate::error::AppError;
use crate::persist;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ── Embedded skill content (compile-time via include_str!) ──

const SKILL_ORCHESTRATOR: &str =
    include_str!("../../templates/skills/loopdeck-orchestrator/SKILL.md");
const SKILL_RUST_EXPERT: &str =
    include_str!("../../templates/skills/loopdeck-rust-expert/SKILL.md");
const SKILL_RUST_CODE_REVIEWER: &str =
    include_str!("../../templates/skills/loopdeck-rust-code-reviewer/SKILL.md");
const SKILL_VITE_SENIOR_ENGINEER: &str =
    include_str!("../../templates/skills/loopdeck-vite-senior-engineer/SKILL.md");
const SKILL_GO_DEV: &str = include_str!("../../templates/skills/loopdeck-go-dev/SKILL.md");
const SKILL_GO_CODE_REVIEW: &str =
    include_str!("../../templates/skills/loopdeck-go-code-review/SKILL.md");
const SKILL_IOS_DEV: &str = include_str!("../../templates/skills/loopdeck-ios-dev/SKILL.md");
const SKILL_IOS_CODE_REVIEW: &str =
    include_str!("../../templates/skills/loopdeck-ios-code-review/SKILL.md");
const SKILL_TAURI_EXPERT: &str =
    include_str!("../../templates/skills/loopdeck-tauri-expert/SKILL.md");
const SKILL_TAURI_CODE_REVIEWER: &str =
    include_str!("../../templates/skills/loopdeck-tauri-code-reviewer/SKILL.md");
const SKILL_API_EXPERT: &str = include_str!("../../templates/skills/loopdeck-api-expert/SKILL.md");
const SKILL_LOOP_RUNNER: &str =
    include_str!("../../templates/skills/loopdeck-loop-runner/SKILL.md");
const SKILL_EPIC_AUTHOR: &str =
    include_str!("../../templates/skills/loopdeck-epic-author/SKILL.md");
const SKILL_MEMORY: &str = include_str!("../../templates/skills/loopdeck-memory/SKILL.md");
const SKILL_OPEN_PR: &str = include_str!("../../templates/skills/loopdeck-open-pr/SKILL.md");
const SKILL_PRD_VERIFIER: &str =
    include_str!("../../templates/skills/loopdeck-prd-verifier/SKILL.md");

// ── Embedded hook scripts ──

const HOOK_STOP_PY: &str = include_str!("../../templates/hooks/loopdeck-stop-hook.py");
const HOOK_MEMORY_WRITE_SH: &str = include_str!("../../templates/hooks/loopdeck-memory-write.sh");
const HOOK_ORCHESTRATOR_START_PY: &str =
    include_str!("../../templates/hooks/orchestrator-start.py");
const HOOK_DECISIONS_CAP_PY: &str = include_str!("../../templates/hooks/loopdeck-decisions-cap.py");
const HOOK_DIRTY_FLAG_PY: &str = include_str!("../../templates/hooks/loopdeck-dirty-flag.py");

// ── Skill name constants (directory names) ──

const NAME_ORCHESTRATOR: &str = "loopdeck-orchestrator";
const NAME_RUST_EXPERT: &str = "loopdeck-rust-expert";
const NAME_RUST_CODE_REVIEWER: &str = "loopdeck-rust-code-reviewer";
const NAME_VITE_SENIOR_ENGINEER: &str = "loopdeck-vite-senior-engineer";
const NAME_GO_DEV: &str = "loopdeck-go-dev";
const NAME_GO_CODE_REVIEW: &str = "loopdeck-go-code-review";
const NAME_IOS_DEV: &str = "loopdeck-ios-dev";
const NAME_IOS_CODE_REVIEW: &str = "loopdeck-ios-code-review";
const NAME_TAURI_EXPERT: &str = "loopdeck-tauri-expert";
const NAME_TAURI_CODE_REVIEWER: &str = "loopdeck-tauri-code-reviewer";
const NAME_API_EXPERT: &str = "loopdeck-api-expert";
const NAME_LOOP_RUNNER: &str = "loopdeck-loop-runner";
const NAME_EPIC_AUTHOR: &str = "loopdeck-epic-author";
const NAME_MEMORY: &str = "loopdeck-memory";
const NAME_OPEN_PR: &str = "loopdeck-open-pr";
const NAME_PRD_VERIFIER: &str = "loopdeck-prd-verifier";

// ── Lookup from skill name to embedded content ──

fn skill_content(name: &str) -> Option<&'static str> {
    match name {
        NAME_ORCHESTRATOR => Some(SKILL_ORCHESTRATOR),
        NAME_RUST_EXPERT => Some(SKILL_RUST_EXPERT),
        NAME_RUST_CODE_REVIEWER => Some(SKILL_RUST_CODE_REVIEWER),
        NAME_VITE_SENIOR_ENGINEER => Some(SKILL_VITE_SENIOR_ENGINEER),
        NAME_GO_DEV => Some(SKILL_GO_DEV),
        NAME_GO_CODE_REVIEW => Some(SKILL_GO_CODE_REVIEW),
        NAME_IOS_DEV => Some(SKILL_IOS_DEV),
        NAME_IOS_CODE_REVIEW => Some(SKILL_IOS_CODE_REVIEW),
        NAME_TAURI_EXPERT => Some(SKILL_TAURI_EXPERT),
        NAME_TAURI_CODE_REVIEWER => Some(SKILL_TAURI_CODE_REVIEWER),
        NAME_API_EXPERT => Some(SKILL_API_EXPERT),
        NAME_LOOP_RUNNER => Some(SKILL_LOOP_RUNNER),
        NAME_EPIC_AUTHOR => Some(SKILL_EPIC_AUTHOR),
        NAME_MEMORY => Some(SKILL_MEMORY),
        NAME_OPEN_PR => Some(SKILL_OPEN_PR),
        NAME_PRD_VERIFIER => Some(SKILL_PRD_VERIFIER),
        _ => None,
    }
}

/// Check whether a marker string matches an Xcode project file pattern.
fn is_xcode_marker(marker: &str) -> bool {
    marker.ends_with(".xcodeproj") || marker.ends_with(".xcworkspace")
}

/// Check whether a marker represents a backend/API stack.
fn is_backend_marker(marker: &str) -> bool {
    matches!(
        marker,
        "Cargo.toml" | "go.mod" | "Package.swift" | "Gemfile" | "Podfile"
    ) || is_xcode_marker(marker)
}

/// Determine which skill directory names apply to a project given its
/// marker files and filesystem structure.
///
/// Returns a sorted, deduplicated list.
pub fn determine_skills(repo_path: &Path, markers: &[String]) -> Vec<String> {
    let mut skills: HashSet<&str> = HashSet::new();

    // Always-installed skills (mechanics + orchestration), regardless of stack.
    // The orchestrator drives multi-agent feature flows; loop-runner executes a
    // single loop; epic-author drafts specs; memory holds the .loopdeck/ write
    // conventions; open-pr + prd-verifier are the orchestrator's verify→ship tail.
    skills.insert(NAME_ORCHESTRATOR);
    skills.insert(NAME_LOOP_RUNNER);
    skills.insert(NAME_EPIC_AUTHOR);
    skills.insert(NAME_MEMORY);
    skills.insert(NAME_OPEN_PR);
    skills.insert(NAME_PRD_VERIFIER);

    let has_cargo = markers.iter().any(|m| m == "Cargo.toml");
    let has_package_json = markers.iter().any(|m| m == "package.json");
    let has_go_mod = markers.iter().any(|m| m == "go.mod");
    let has_swift = markers.iter().any(|m| m == "Package.swift");
    let has_podfile = markers.iter().any(|m| m == "Podfile");
    let has_xcode = markers.iter().any(|m| is_xcode_marker(m));
    let has_any_ios = has_swift || has_podfile || has_xcode;

    // Rust
    if has_cargo {
        skills.insert(NAME_RUST_EXPERT);
        skills.insert(NAME_RUST_CODE_REVIEWER);
    }

    // JavaScript / TypeScript (Vite / frontend)
    if has_package_json {
        skills.insert(NAME_VITE_SENIOR_ENGINEER);
    }

    // Go
    if has_go_mod {
        skills.insert(NAME_GO_DEV);
        skills.insert(NAME_GO_CODE_REVIEW);
    }

    // iOS / Swift
    if has_any_ios {
        skills.insert(NAME_IOS_DEV);
        skills.insert(NAME_IOS_CODE_REVIEW);
    }

    // Tauri: must have both Cargo + package.json AND a src-tauri/ directory
    if has_cargo && has_package_json && repo_path.join("src-tauri").is_dir() {
        skills.insert(NAME_TAURI_EXPERT);
        skills.insert(NAME_TAURI_CODE_REVIEWER);
    }

    // API expert: any backend marker OR an api/ directory
    let has_backend_marker = markers.iter().any(|m| is_backend_marker(m));
    if has_backend_marker || repo_path.join("api").is_dir() {
        skills.insert(NAME_API_EXPERT);
    }

    // Return sorted Vec
    let mut result: Vec<String> = skills.into_iter().map(String::from).collect();
    result.sort();
    result
}

// ── App version + managed-skills manifest ──
//
// The `loopdeck-` prefix is the skills ownership boundary (ADR-4 of the
// `support-project-management` epic): app-managed skills may be overwritten
// when the app version advances; user-owned skills (no prefix) are never
// touched. Each harness skill root has its own per-project manifest
// (`.claude/skills/.loopdeck-manifest.json` for Claude Code and
// `.agents/skills/.loopdeck-manifest.json` for Codex), so a refresh only
// overwrites managed skills when the version actually moved.

/// The app version, baked in at compile time from `Cargo.toml`'s `version`.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Record of which skills the app installed and at what version.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SkillManifest {
    #[serde(default)]
    version: String,
    #[serde(default)]
    skills: Vec<String>,
}

impl Default for SkillManifest {
    fn default() -> Self {
        // A manifest-less project is treated as version "0.0.0" so the very
        // first refresh (app version > 0.0.0) overwrites every managed skill.
        SkillManifest {
            version: "0.0.0".to_string(),
            skills: Vec::new(),
        }
    }
}

fn manifest_path(skills_dir: &Path) -> std::path::PathBuf {
    skills_dir.join(".loopdeck-manifest.json")
}

/// Read the project's skill manifest; a missing or malformed file yields the
/// default (`version: "0.0.0"`), never an error — a bad manifest must not
/// block a refresh.
fn read_manifest(skills_dir: &Path) -> SkillManifest {
    let path = manifest_path(skills_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str::<SkillManifest>(&raw).unwrap_or_default(),
        Err(_) => SkillManifest::default(),
    }
}

fn write_manifest(skills_dir: &Path, manifest: &SkillManifest) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| AppError::Config(format!("manifest serialization error: {e}")))?;
    let path = manifest_path(skills_dir);
    persist::atomic_write(&path, &json)?;
    Ok(())
}

/// Parse a `major.minor.patch` version into a tuple. Malformed or missing
/// components default to 0, so a garbage manifest version compares as `0.0.0`
/// and never blocks a refresh.
fn parse_version(v: &str) -> (u32, u32, u32) {
    let mut parts = v.split('.');
    let maj = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let min = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let pat = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (maj, min, pat)
}

/// True iff `a` is a strictly newer `major.minor.patch` than `b` (numeric, not
/// lexicographic — so `0.10.0` > `0.9.0`).
fn version_gt(a: &str, b: &str) -> bool {
    parse_version(a) > parse_version(b)
}

/// Return the project skill directory discovered by the selected harness.
pub(crate) fn skills_dir_for_harness(repo_path: &Path, harness: AgentHarness) -> PathBuf {
    match harness {
        AgentHarness::Claude => repo_path.join(".claude").join("skills"),
        AgentHarness::Codex => repo_path.join(".agents").join("skills"),
    }
}

/// Copy the project's skill `SKILL.md` files into
/// both harness-native roots (`.claude/skills` and `.agents/skills`), using a
/// version-aware refresh. Codex discovers repository skills only from
/// `.agents/skills`; keeping the two installations in sync lets a project
/// switch harnesses without another bootstrap.
///
/// Returns the list of skill names that apply to the project (whether or not
/// each was freshly written).
///
/// **Refresh rule.** For each applicable skill:
/// - if it's app-managed (`loopdeck-` prefix) **and** the app version advanced
///   past the manifest's recorded version → **overwrite**;
/// - if it's app-managed at the same version and already exists → **skip**
///   (don't clobber);
/// - otherwise (already exists) → **skip**.
///
/// A managed skill that does not yet exist is always installed, even at the
/// same version. After writing, the manifest is updated to the current app
/// version + skill set. User-owned (non-`loopdeck-`) skills are never in
/// `determine_skills`' output and are never touched.
pub fn copy_skills(repo_path: &Path, markers: &[String]) -> Result<Vec<String>, AppError> {
    copy_skills_with_version(repo_path, markers, APP_VERSION)
}

/// Injectable inner form — tests pass an explicit version rather than the
/// compile-time `APP_VERSION`.
fn copy_skills_with_version(
    repo_path: &Path,
    markers: &[String],
    app_version: &str,
) -> Result<Vec<String>, AppError> {
    let names = determine_skills(repo_path, markers);
    for harness in [AgentHarness::Claude, AgentHarness::Codex] {
        let skills_dir = skills_dir_for_harness(repo_path, harness);
        std::fs::create_dir_all(&skills_dir)?;

        let manifest = read_manifest(&skills_dir);
        let overwrite_managed = version_gt(app_version, &manifest.version);

        for name in &names {
            let skill_dir = skills_dir.join(name);
            let skill_file = skill_dir.join("SKILL.md");

            let is_managed = name.starts_with("loopdeck-");
            let exists = skill_file.exists();

            // Skip when there's nothing to do: an existing managed skill at
            // the same app version, or any already-present user-owned skill.
            if exists && !(is_managed && overwrite_managed) {
                continue;
            }

            if let Some(content) = skill_content(name) {
                std::fs::create_dir_all(&skill_dir)?;
                std::fs::write(&skill_file, content)?;
            }
        }

        // Each harness root owns its refresh state independently. This also
        // backfills `.agents/skills` for an existing Claude-only project even
        // when `.claude/skills` is already at the current app version.
        write_manifest(
            &skills_dir,
            &SkillManifest {
                version: app_version.to_string(),
                skills: names.clone(),
            },
        )?;
    }

    Ok(names)
}

/// Set up Claude Code hooks in `<repo_path>/.claude/settings.json` and copy
/// hook scripts into `<repo_path>/.claude/`.
///
/// # Hook entries created
///
/// | Event       | Command                                              | Matcher                |
/// |-------------|-------------------------------------------------------|------------------------|
/// | Stop        | `python3 .loopdeck/hooks/loopdeck-stop-hook.py`        | (none)                 |
/// | Stop        | `bash .loopdeck/hooks/loopdeck-memory-write.sh`        | (none)                 |
/// | PreToolUse  | `python3 .loopdeck/hooks/orchestrator-start.py`        | `Skill`                |
/// | PreToolUse  | `python3 .loopdeck/hooks/loopdeck-dirty-flag.py`       | `Edit\|Write\|MultiEdit\|NotebookEdit` |
/// | PostToolUse | `python3 .loopdeck/hooks/loopdeck-decisions-cap.py`    | `Edit\|Write\|MultiEdit` |
///
/// # Deduplication
///
/// Before inserting a hook, the existing entries for that event are checked.
/// If a hook with the same `command` string already exists in any matcher
/// group, it is skipped.
pub fn setup_hooks(repo_path: &Path) -> Result<(), AppError> {
    let claude_dir = repo_path.join(".claude");
    let loopdeck_hooks_dir = repo_path.join(".loopdeck").join("hooks");

    std::fs::create_dir_all(&claude_dir)?;
    std::fs::create_dir_all(&loopdeck_hooks_dir)?;

    // ── Copy hook scripts into .loopdeck/hooks/ (idempotent) ──

    let stop_hook_path = loopdeck_hooks_dir.join("loopdeck-stop-hook.py");
    if !stop_hook_path.exists() {
        std::fs::write(&stop_hook_path, HOOK_STOP_PY)?;
    }

    let memory_write_path = loopdeck_hooks_dir.join("loopdeck-memory-write.sh");
    if !memory_write_path.exists() {
        std::fs::write(&memory_write_path, HOOK_MEMORY_WRITE_SH)?;
    }

    let orchestrator_path = loopdeck_hooks_dir.join("orchestrator-start.py");
    if !orchestrator_path.exists() {
        std::fs::write(&orchestrator_path, HOOK_ORCHESTRATOR_START_PY)?;
    }

    let decisions_cap_path = loopdeck_hooks_dir.join("loopdeck-decisions-cap.py");
    if !decisions_cap_path.exists() {
        std::fs::write(&decisions_cap_path, HOOK_DECISIONS_CAP_PY)?;
    }

    let dirty_flag_path = loopdeck_hooks_dir.join("loopdeck-dirty-flag.py");
    if !dirty_flag_path.exists() {
        std::fs::write(&dirty_flag_path, HOOK_DIRTY_FLAG_PY)?;
    }

    // ── Build / update .claude/settings.json ──

    let settings_path = claude_dir.join("settings.json");

    let mut root: serde_json::Value = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Ensure hooks object exists
    if root.get("hooks").is_none() {
        root["hooks"] = serde_json::json!({});
    }

    // ── Stop hooks (single matcher group, no matcher) ──

    {
        let hooks = &mut root["hooks"];

        // Ensure hooks.Stop array exists
        if hooks.get("Stop").is_none() {
            hooks["Stop"] = serde_json::json!([]);
        }

        // Find or create the default (no-matcher) Stop group
        let group_idx = find_or_create_matcher_group(&mut hooks["Stop"], "");

        let stop_commands = [
            "python3 .loopdeck/hooks/loopdeck-stop-hook.py",
            "bash .loopdeck/hooks/loopdeck-memory-write.sh",
        ];

        if let Some(arr) = hooks["Stop"].as_array_mut() {
            if let Some(group) = arr.get_mut(group_idx) {
                for cmd in &stop_commands {
                    if !hook_command_exists_in_group(group, cmd) {
                        if let Some(inner) = group["hooks"].as_array_mut() {
                            inner.push(serde_json::json!({
                                "type": "command",
                                "command": cmd
                            }));
                        }
                    }
                }
            }
        }
    }

    // ── PreToolUse hook (matcher: "Skill") ──

    {
        let hooks = &mut root["hooks"];

        if hooks.get("PreToolUse").is_none() {
            hooks["PreToolUse"] = serde_json::json!([]);
        }

        let group_idx = find_or_create_matcher_group(&mut hooks["PreToolUse"], "Skill");

        let orch_cmd = "python3 .loopdeck/hooks/orchestrator-start.py";

        if let Some(arr) = hooks["PreToolUse"].as_array_mut() {
            if let Some(group) = arr.get_mut(group_idx) {
                if !hook_command_exists_in_group(group, orch_cmd) {
                    if let Some(inner) = group["hooks"].as_array_mut() {
                        inner.push(serde_json::json!({
                            "type": "command",
                            "command": orch_cmd
                        }));
                    }
                }
            }
        }
    }

    // ── PreToolUse hook (matcher: "Edit|Write|MultiEdit|NotebookEdit") ──
    // Creates .claude/.session-dirty so the Stop hook's memory nudge fires.
    // Without this, loopdeck-stop-hook.py's DIRTY_FILE check is always
    // false and the reminder never emits — see loopdeck-stop-hook.py's
    // module docstring for the full gating chain.

    {
        let hooks = &mut root["hooks"];

        let group_idx = find_or_create_matcher_group(
            &mut hooks["PreToolUse"],
            "Edit|Write|MultiEdit|NotebookEdit",
        );

        let dirty_cmd = "python3 .loopdeck/hooks/loopdeck-dirty-flag.py";

        if let Some(arr) = hooks["PreToolUse"].as_array_mut() {
            if let Some(group) = arr.get_mut(group_idx) {
                if !hook_command_exists_in_group(group, dirty_cmd) {
                    if let Some(inner) = group["hooks"].as_array_mut() {
                        inner.push(serde_json::json!({
                            "type": "command",
                            "command": dirty_cmd
                        }));
                    }
                }
            }
        }
    }

    // ── PostToolUse hook (matcher: "Edit|Write|MultiEdit") ──

    {
        let hooks = &mut root["hooks"];

        if hooks.get("PostToolUse").is_none() {
            hooks["PostToolUse"] = serde_json::json!([]);
        }

        let group_idx =
            find_or_create_matcher_group(&mut hooks["PostToolUse"], "Edit|Write|MultiEdit");

        let cap_cmd = "python3 .loopdeck/hooks/loopdeck-decisions-cap.py";

        if let Some(arr) = hooks["PostToolUse"].as_array_mut() {
            if let Some(group) = arr.get_mut(group_idx) {
                if !hook_command_exists_in_group(group, cap_cmd) {
                    if let Some(inner) = group["hooks"].as_array_mut() {
                        inner.push(serde_json::json!({
                            "type": "command",
                            "command": cap_cmd
                        }));
                    }
                }
            }
        }
    }

    // ── permissions.allow list (curated safe-command allowlist) ──
    //
    // Under `--permission-mode default`, a tool call only emits a
    // `control_request` if it doesn't match an allow rule. Seeding a curated
    // list of known-safe read commands means the common dev-loop traffic
    // (ls, git status, file reads) short-circuits the control protocol
    // entirely — so it neither stalls nor clutters the permission log/UI.
    // Anything not on this list routes through LoopDeck's policy
    // (destructive floor -> MANUAL_APPROVAL_TOOLS interception -> auto-allow).
    //
    // **What is deliberately NOT here (Phase 1, PRD FR1):**
    // - No `Edit(*)` / `Write(*)` / `NotebookEdit(*)` — file mutation must
    //   route through the manual-approval card so the user sees it.
    // - No broad build-runner rules (`Bash(cargo:*)`, `Bash(npm:*)`, etc.) —
    //   a hostile repo controls its own scripts and build steps, so a broad
    //   allow rule is a privilege escalation vector. Users who trust a
    //   project can add narrow rules (e.g. `Bash(npm run test:*)`) via the
    //   approval card's "Always allow" button, which writes to
    //   `.claude/settings.local.json`.
    // Idempotent + dedup'd, mirroring the hook-insertion pattern above; any
    // pre-existing rules are preserved.
    {
        const CURATED_ALLOW: &[&str] = &[
            // Read-only filesystem tools.
            "Read(*)",
            "Glob(*)",
            "Grep(*)",
            // Common safe read-only Bash commands.
            "Bash(ls:*)",
            "Bash(cat:*)",
            "Bash(pwd)",
            "Bash(which:*)",
            "Bash(find:*)",
            "Bash(grep:*)",
            "Bash(rg:*)",
            "Bash(echo:*)",
            "Bash(mkdir:-p:*)",
            // VCS inspection (no state changes except git add — staging is
            // reversible and the orchestrator needs it for its write-through).
            "Bash(git status:*)",
            "Bash(git diff:*)",
            "Bash(git log:*)",
            "Bash(git show:*)",
            "Bash(git branch:*)",
            "Bash(git add:*)",
        ];

        if root.get("permissions").is_none() {
            root["permissions"] = serde_json::json!({});
        }
        let allow_arr = root["permissions"]["allow"]
            .as_array_mut()
            .map(std::mem::take)
            .unwrap_or_default();
        let mut existing: Vec<serde_json::Value> = allow_arr;
        for rule in CURATED_ALLOW {
            if !existing.iter().any(|v| v.as_str() == Some(*rule)) {
                existing.push(serde_json::Value::from(*rule));
            }
        }
        root["permissions"]["allow"] = serde_json::Value::Array(existing);
    }

    // Write settings.json (pretty-printed for readability) atomically so a
    // crash mid-write can't leave a truncated allowlist — a half-written
    // settings.json could either drop the user's curated rules or leave
    // malformed JSON that claude refuses to load.
    let formatted = serde_json::to_string_pretty(&root)
        .map_err(|e| AppError::Config(format!("JSON serialization error: {e}")))?;
    persist::atomic_write(&settings_path, &formatted)?;

    Ok(())
}

/// Find or create a matcher group in a hook event array.
/// Returns the index of the group with the given `matcher` value.
///
/// An empty matcher string `""` denotes the default (no-matcher) group.
fn find_or_create_matcher_group(event_array: &mut serde_json::Value, matcher: &str) -> usize {
    // A pre-existing user `.claude/settings.json` may hold a hook event keyed to
    // a non-array value (a string, number, object — hand-edited or written by a
    // different tool). Coerce it to an empty array rather than panicking: under
    // `panic = "abort"` a panic here aborts the whole process on import, and
    // `setup_hooks` is on the import path. This mirrors the lenient parse
    // fallback above (line ~213: malformed JSON → `{}`): treat malformed user
    // config as "no existing hooks" and proceed.
    if !event_array.is_array() {
        *event_array = serde_json::Value::Array(Vec::new());
    }
    let arr = event_array
        .as_array_mut()
        .expect("coerced to an array immediately above");

    // Look for an existing group with the same matcher
    for (i, entry) in arr.iter().enumerate() {
        let entry_matcher = entry.get("matcher").and_then(|v| v.as_str()).unwrap_or("");
        if entry_matcher == matcher {
            return i;
        }
    }

    // Not found — create a new group
    let mut group = serde_json::json!({
        "hooks": []
    });
    if !matcher.is_empty() {
        group["matcher"] = serde_json::Value::String(matcher.to_string());
    }
    arr.push(group);
    arr.len() - 1
}

/// Check whether a command hook already exists inside a matcher group's `hooks` array.
fn hook_command_exists_in_group(group: &serde_json::Value, command: &str) -> bool {
    group
        .get("hooks")
        .and_then(|v| v.as_array())
        .map(|hooks| {
            hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(command))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("loopdeck-skills-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ── determine_skills tests ──

    #[test]
    fn test_core_skills_always_included() {
        let dir = temp_dir();
        let skills = determine_skills(&dir, &[]);
        // Mechanics + orchestration skills are always installed, regardless of stack.
        assert!(skills.contains(&NAME_ORCHESTRATOR.to_string()));
        assert!(skills.contains(&NAME_LOOP_RUNNER.to_string()));
        assert!(skills.contains(&NAME_EPIC_AUTHOR.to_string()));
        assert!(skills.contains(&NAME_MEMORY.to_string()));
        assert!(skills.contains(&NAME_OPEN_PR.to_string()));
        assert!(skills.contains(&NAME_PRD_VERIFIER.to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_rust_markers() {
        let dir = temp_dir();
        let markers = vec!["Cargo.toml".to_string()];
        let skills = determine_skills(&dir, &markers);
        assert!(skills.contains(&NAME_RUST_EXPERT.to_string()));
        assert!(skills.contains(&NAME_RUST_CODE_REVIEWER.to_string()));
        assert!(skills.contains(&NAME_API_EXPERT.to_string())); // backend marker
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_package_json() {
        let dir = temp_dir();
        let markers = vec!["package.json".to_string()];
        let skills = determine_skills(&dir, &markers);
        assert!(skills.contains(&NAME_VITE_SENIOR_ENGINEER.to_string()));
        assert!(!skills.contains(&NAME_TAURI_EXPERT.to_string())); // no Cargo, no src-tauri/
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_go_markers() {
        let dir = temp_dir();
        let markers = vec!["go.mod".to_string()];
        let skills = determine_skills(&dir, &markers);
        assert!(skills.contains(&NAME_GO_DEV.to_string()));
        assert!(skills.contains(&NAME_GO_CODE_REVIEW.to_string()));
        assert!(skills.contains(&NAME_API_EXPERT.to_string())); // backend marker
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_ios_markers() {
        let dir = temp_dir();
        let markers = vec![
            "Package.swift".to_string(),
            "Podfile".to_string(),
            "MyApp.xcodeproj".to_string(),
        ];
        let skills = determine_skills(&dir, &markers);
        assert!(skills.contains(&NAME_IOS_DEV.to_string()));
        assert!(skills.contains(&NAME_IOS_CODE_REVIEW.to_string()));
        assert!(skills.contains(&NAME_API_EXPERT.to_string())); // backend marker
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_tauri_detection_with_src_tauri_dir() {
        let dir = temp_dir();
        // Create src-tauri/ directory so Tauri detection passes
        fs::create_dir_all(dir.join("src-tauri")).unwrap();
        let markers = vec!["Cargo.toml".to_string(), "package.json".to_string()];
        let skills = determine_skills(&dir, &markers);
        assert!(skills.contains(&NAME_TAURI_EXPERT.to_string()));
        assert!(skills.contains(&NAME_TAURI_CODE_REVIEWER.to_string()));
        assert!(skills.contains(&NAME_RUST_EXPERT.to_string()));
        assert!(skills.contains(&NAME_VITE_SENIOR_ENGINEER.to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_tauri_detection_without_src_tauri_dir() {
        let dir = temp_dir();
        // No src-tauri/ directory
        let markers = vec!["Cargo.toml".to_string(), "package.json".to_string()];
        let skills = determine_skills(&dir, &markers);
        assert!(!skills.contains(&NAME_TAURI_EXPERT.to_string()));
        assert!(!skills.contains(&NAME_TAURI_CODE_REVIEWER.to_string()));
        // Still gets Rust + Vite
        assert!(skills.contains(&NAME_RUST_EXPERT.to_string()));
        assert!(skills.contains(&NAME_VITE_SENIOR_ENGINEER.to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_api_expert_from_api_directory() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("api")).unwrap();
        // No backend markers, but api/ dir exists
        let markers = vec!["package.json".to_string()];
        let skills = determine_skills(&dir, &markers);
        assert!(skills.contains(&NAME_API_EXPERT.to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_no_duplicates() {
        let dir = temp_dir();
        // Duplicate markers should not produce duplicate skill names
        let markers = vec![
            "Cargo.toml".to_string(),
            "Cargo.toml".to_string(),
            "package.json".to_string(),
        ];
        fs::create_dir_all(dir.join("src-tauri")).unwrap();
        let skills = determine_skills(&dir, &markers);
        // Each name should appear exactly once
        let count_rust_expert = skills
            .iter()
            .filter(|s| s.as_str() == NAME_RUST_EXPERT)
            .count();
        assert_eq!(count_rust_expert, 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    // ── copy_skills tests ──

    #[test]
    fn test_copy_skills_creates_files() {
        let dir = temp_dir();
        let markers = vec!["Cargo.toml".to_string()];
        let copied = copy_skills(&dir, &markers).unwrap();
        assert!(!copied.is_empty());

        for harness in [AgentHarness::Claude, AgentHarness::Codex] {
            let skills_dir = skills_dir_for_harness(&dir, harness);

            // Verify orchestrator SKILL.md exists.
            let orch_path = skills_dir.join("loopdeck-orchestrator/SKILL.md");
            assert!(orch_path.exists());
            let content = fs::read_to_string(&orch_path).unwrap();
            assert!(content.contains("name: loopdeck:orchestrator"));

            // Verify the stack-specific skill and manifest also exist.
            assert!(skills_dir.join("loopdeck-rust-expert/SKILL.md").exists());
            assert!(skills_dir.join(".loopdeck-manifest.json").exists());
        }

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_copy_skills_idempotent() {
        let dir = temp_dir();
        let markers = vec!["Cargo.toml".to_string()];

        // First call
        let copied1 = copy_skills(&dir, &markers).unwrap();
        // Second call — should skip existing files
        let copied2 = copy_skills(&dir, &markers).unwrap();

        // copied2 should be the same skill names (all skipped by exists-check)
        assert_eq!(copied1, copied2);

        // Files should still exist and be unchanged
        let rust_path = dir.join(".claude/skills/loopdeck-rust-expert/SKILL.md");
        assert!(rust_path.exists());
        let codex_rust_path = dir.join(".agents/skills/loopdeck-rust-expert/SKILL.md");
        assert!(codex_rust_path.exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_copy_skills_backfills_missing_codex_root() {
        let dir = temp_dir();
        let claude_skills = skills_dir_for_harness(&dir, AgentHarness::Claude);
        let claude_skill = claude_skills.join("loopdeck-memory/SKILL.md");
        fs::create_dir_all(claude_skill.parent().unwrap()).unwrap();
        fs::write(&claude_skill, "KEEP-CLAUDE").unwrap();
        write_manifest(
            &claude_skills,
            &SkillManifest {
                version: "0.2.0".to_string(),
                skills: vec!["loopdeck-memory".to_string()],
            },
        )
        .unwrap();

        copy_skills_with_version(&dir, &[], "0.2.0").unwrap();

        assert_eq!(fs::read_to_string(&claude_skill).unwrap(), "KEEP-CLAUDE");
        assert!(skills_dir_for_harness(&dir, AgentHarness::Codex)
            .join("loopdeck-memory/SKILL.md")
            .exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    // ── version-aware refresh + manifest tests ──

    #[test]
    fn test_version_refresh_overwrites_managed_skill() {
        let dir = temp_dir();
        let skills_dir = dir.join(".claude").join("skills");
        let orch_file = skills_dir.join("loopdeck-orchestrator/SKILL.md");
        fs::create_dir_all(orch_file.parent().unwrap()).unwrap();
        // Seed a stale managed skill + a manifest recording an older version.
        fs::write(&orch_file, "STALE-CONTENT").unwrap();
        write_manifest(
            &skills_dir,
            &SkillManifest {
                version: "0.1.0".to_string(),
                skills: vec![],
            },
        )
        .unwrap();

        // App 0.2.0 > manifest 0.1.0 → managed skill is overwritten.
        copy_skills_with_version(&dir, &[], "0.2.0").unwrap();
        let after = fs::read_to_string(&orch_file).unwrap();
        assert_ne!(after, "STALE-CONTENT");
        assert!(after.contains("name: loopdeck:orchestrator"));

        // Manifest now records the new version.
        assert_eq!(read_manifest(&skills_dir).version, "0.2.0");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_version_equal_skips_existing_managed_skill() {
        let dir = temp_dir();
        let skills_dir = dir.join(".claude").join("skills");
        let orch_file = skills_dir.join("loopdeck-orchestrator/SKILL.md");
        fs::create_dir_all(orch_file.parent().unwrap()).unwrap();
        fs::write(&orch_file, "KEEP-ME").unwrap();
        write_manifest(
            &skills_dir,
            &SkillManifest {
                version: "0.2.0".to_string(),
                skills: vec![],
            },
        )
        .unwrap();

        // Same version → existing managed skill is NOT overwritten.
        copy_skills_with_version(&dir, &[], "0.2.0").unwrap();
        assert_eq!(fs::read_to_string(&orch_file).unwrap(), "KEEP-ME");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_user_owned_skill_untouched_on_refresh() {
        // A non-`loopdeck-`-prefixed skill is user-owned; it is never returned
        // by determine_skills and must survive a version-advancing refresh.
        let dir = temp_dir();
        let skills_dir = dir.join(".claude").join("skills");
        let user_file = skills_dir.join("my-custom-skill/SKILL.md");
        fs::create_dir_all(user_file.parent().unwrap()).unwrap();
        fs::write(&user_file, "USER-OWNED").unwrap();
        write_manifest(
            &skills_dir,
            &SkillManifest {
                version: "0.1.0".to_string(),
                skills: vec![],
            },
        )
        .unwrap();

        copy_skills_with_version(&dir, &[], "0.2.0").unwrap();
        assert_eq!(fs::read_to_string(&user_file).unwrap(), "USER-OWNED");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_manifest_round_trip() {
        let dir = temp_dir();
        let skills_dir = dir.join(".claude").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();
        let m = SkillManifest {
            version: "0.2.0".to_string(),
            skills: vec!["loopdeck-orchestrator".to_string()],
        };
        write_manifest(&skills_dir, &m).unwrap();
        let read = read_manifest(&skills_dir);
        assert_eq!(read.version, "0.2.0");
        assert_eq!(read.skills, vec!["loopdeck-orchestrator".to_string()]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_manifest_missing_defaults_to_zero() {
        let dir = temp_dir();
        let skills_dir = dir.join(".claude").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();
        let m = read_manifest(&skills_dir);
        assert_eq!(m.version, "0.0.0");
        assert!(m.skills.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_version_gt_is_numeric_semver() {
        assert!(version_gt("0.2.0", "0.1.0"));
        assert!(version_gt("1.0.0", "0.9.9"));
        assert!(version_gt("0.10.0", "0.9.0")); // numeric, not lexicographic
        assert!(!version_gt("0.2.0", "0.2.0"));
        assert!(!version_gt("0.1.0", "0.2.0"));
        assert!(version_gt("0.0.1", "garbage")); // malformed → 0.0.0
        assert!(!version_gt("garbage", "0.0.1"));
    }

    // ── setup_hooks tests ──

    #[test]
    fn test_setup_hooks_creates_settings_json() {
        let dir = temp_dir();
        setup_hooks(&dir).unwrap();

        let settings_path = dir.join(".claude/settings.json");
        assert!(settings_path.exists());

        let raw = fs::read_to_string(&settings_path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();

        // Check Stop hooks — one matcher group, two command hooks inside
        let stop_groups = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop_groups.len(), 1);
        let stop_hooks = stop_groups[0]["hooks"].as_array().unwrap();
        assert_eq!(stop_hooks.len(), 2);

        let stop_commands: Vec<&str> = stop_hooks
            .iter()
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert!(stop_commands.contains(&"python3 .loopdeck/hooks/loopdeck-stop-hook.py"));
        assert!(stop_commands.contains(&"bash .loopdeck/hooks/loopdeck-memory-write.sh"));
        // Each hook entry should have type "command"
        for h in stop_hooks {
            assert_eq!(h["type"].as_str().unwrap(), "command");
        }

        // Check PreToolUse hooks — two matcher groups: "Skill" and the dirty-flag one
        let ptuse_groups = root["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(ptuse_groups[0]["matcher"].as_str().unwrap(), "Skill");
        let ptuse_hooks = ptuse_groups[0]["hooks"].as_array().unwrap();
        assert_eq!(ptuse_hooks.len(), 1);
        assert_eq!(
            ptuse_hooks[0]["command"].as_str().unwrap(),
            "python3 .loopdeck/hooks/orchestrator-start.py"
        );
        assert_eq!(ptuse_hooks[0]["type"].as_str().unwrap(), "command");

        // Check the dirty-flag PreToolUse group — second matcher group
        assert_eq!(ptuse_groups.len(), 2);
        assert_eq!(
            ptuse_groups[1]["matcher"].as_str().unwrap(),
            "Edit|Write|MultiEdit|NotebookEdit"
        );
        let dirty_hooks = ptuse_groups[1]["hooks"].as_array().unwrap();
        assert_eq!(dirty_hooks.len(), 1);
        assert_eq!(
            dirty_hooks[0]["command"].as_str().unwrap(),
            "python3 .loopdeck/hooks/loopdeck-dirty-flag.py"
        );

        // Check PostToolUse hook — one matcher group with matcher "Edit|Write|MultiEdit"
        let ptu_groups = root["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(ptu_groups.len(), 1);
        assert_eq!(
            ptu_groups[0]["matcher"].as_str().unwrap(),
            "Edit|Write|MultiEdit"
        );
        let ptu_hooks = ptu_groups[0]["hooks"].as_array().unwrap();
        assert_eq!(ptu_hooks.len(), 1);
        assert_eq!(
            ptu_hooks[0]["command"].as_str().unwrap(),
            "python3 .loopdeck/hooks/loopdeck-decisions-cap.py"
        );
        assert_eq!(ptu_hooks[0]["type"].as_str().unwrap(), "command");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_setup_hooks_dedup() {
        let dir = temp_dir();

        // First call
        setup_hooks(&dir).unwrap();

        // Second call — should not duplicate
        setup_hooks(&dir).unwrap();

        let raw = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();

        // Still only 1 Stop matcher group with 2 hooks
        let stop_groups = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop_groups.len(), 1);
        assert_eq!(stop_groups[0]["hooks"].as_array().unwrap().len(), 2);

        // Still only 2 PreToolUse matcher groups, 1 hook each
        let ptuse_groups = root["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(ptuse_groups.len(), 2);
        assert_eq!(ptuse_groups[0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(ptuse_groups[1]["hooks"].as_array().unwrap().len(), 1);

        // Still only 1 PostToolUse matcher group with 1 hook
        let ptu_groups = root["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(ptu_groups.len(), 1);
        assert_eq!(ptu_groups[0]["hooks"].as_array().unwrap().len(), 1);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_setup_hooks_preserves_existing_permissions() {
        let dir = temp_dir();

        // Pre-create a settings.json with permissions
        let existing = serde_json::json!({
            "permissions": {
                "allow": ["Read(*)", "Write(*)"]
            }
        });
        fs::create_dir_all(dir.join(".claude")).unwrap();
        fs::write(
            dir.join(".claude/settings.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        // Now run setup_hooks
        setup_hooks(&dir).unwrap();

        let raw = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();

        // Permissions should still be intact
        let perms = root["permissions"]["allow"].as_array().unwrap();
        assert!(perms.iter().any(|v| v.as_str().unwrap() == "Read(*)"));
        assert!(perms.iter().any(|v| v.as_str().unwrap() == "Write(*)"));

        // Hooks should also be present
        assert!(root["hooks"]["Stop"].as_array().unwrap().len() > 0);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_setup_hooks_survives_malformed_hook_events() {
        // Regression: a pre-existing `.claude/settings.json` whose hook events
        // are non-arrays (hand-edited, or written by a different tool) must not
        // abort the process. Under `panic = "abort"` the old `.expect()` in
        // `find_or_create_matcher_group` crashed the whole app on import — and
        // `setup_hooks` is on the import path.
        let dir = temp_dir();
        fs::create_dir_all(dir.join(".claude")).unwrap();
        fs::write(
            dir.join(".claude/settings.json"),
            serde_json::json!({
                "hooks": {
                    "Stop": "not-an-array",
                    "PreToolUse": 42
                }
            })
            .to_string(),
        )
        .unwrap();

        setup_hooks(&dir).expect("setup_hooks must tolerate malformed hook events");

        let raw = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();

        // Both malformed values were coerced to arrays and populated.
        let stop_groups = root["hooks"]["Stop"]
            .as_array()
            .expect("malformed Stop coerced to an array");
        assert!(stop_groups[0]["hooks"].as_array().unwrap().iter().any(|h| {
            h["command"].as_str() == Some("python3 .loopdeck/hooks/loopdeck-stop-hook.py")
        }));

        let ptuse_groups = root["hooks"]["PreToolUse"]
            .as_array()
            .expect("malformed PreToolUse coerced to an array");
        assert_eq!(ptuse_groups[0]["matcher"].as_str(), Some("Skill"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_setup_hooks_writes_curated_allowlist() {
        // Under `default` permission mode, a curated allow list lets common
        // safe read commands short-circuit the control protocol. setup_hooks
        // must seed it into every bootstrapped project's settings.json.
        let dir = temp_dir();
        setup_hooks(&dir).unwrap();

        let raw = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let allow = root["permissions"]["allow"]
            .as_array()
            .expect("permissions.allow must exist after setup_hooks");

        // Spot-check a representative sample of what SHOULD be seeded.
        let has = |rule: &str| allow.iter().any(|v| v.as_str() == Some(rule));
        assert!(has("Read(*)"), "read tool missing");
        assert!(has("Bash(ls:*)"), "safe read command missing");
        assert!(has("Bash(git status:*)"), "vcs inspection missing");

        // Phase 1 (PRD FR1): broad mutation + build-runner rules must NOT be
        // seeded — they bypass LoopDeck's manual-approval card at the Claude
        // layer. Users add narrow rules via the approval card's "Always allow"
        // button instead.
        assert!(!has("Edit(*)"), "broad Edit rule must NOT be seeded");
        assert!(!has("Write(*)"), "broad Write rule must NOT be seeded");
        assert!(
            !has("Bash(cargo:*)"),
            "broad build-runner rule must NOT be seeded"
        );
        assert!(
            !has("Bash(npm:*)"),
            "broad build-runner rule must NOT be seeded"
        );
        assert!(
            !has("Bash(npx:*)"),
            "broad build-runner rule must NOT be seeded"
        );
        assert!(
            !has("Bash(go:*)"),
            "broad build-runner rule must NOT be seeded"
        );
        assert!(
            !has("Bash(pnpm:*)"),
            "broad build-runner rule must NOT be seeded"
        );
        assert!(
            !has("Bash(yarn:*)"),
            "broad build-runner rule must NOT be seeded"
        );
        assert!(
            !has("Bash(rm -rf:*)"),
            "destructive patterns must NOT be in the allow list"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_setup_hooks_curated_allowlist_dedups_against_existing() {
        // An existing rule must not be duplicated when the curated list is
        // merged in, and a curated rule must not be added twice on re-run.
        let dir = temp_dir();
        fs::create_dir_all(dir.join(".claude")).unwrap();
        fs::write(
            dir.join(".claude/settings.json"),
            r#"{ "permissions": { "allow": ["Read(*)", "Bash(cargo:*)"] } }"#,
        )
        .unwrap();

        setup_hooks(&dir).unwrap();
        let raw = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let allow = root["permissions"]["allow"].as_array().unwrap();

        let count_read = allow
            .iter()
            .filter(|v| v.as_str() == Some("Read(*)"))
            .count();
        let count_cargo = allow
            .iter()
            .filter(|v| v.as_str() == Some("Bash(cargo:*)"))
            .count();
        assert_eq!(count_read, 1, "pre-existing Read(*) must not be duplicated");
        assert_eq!(
            count_cargo, 1,
            "pre-existing Bash(cargo:*) must not be duplicated"
        );

        // Re-run is also idempotent.
        setup_hooks(&dir).unwrap();
        let raw2 = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
        let root2: serde_json::Value = serde_json::from_str(&raw2).unwrap();
        let allow2 = root2["permissions"]["allow"].as_array().unwrap();
        assert_eq!(
            allow2.len(),
            allow.len(),
            "second setup_hooks run must not add duplicates"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_setup_hooks_copies_scripts() {
        let dir = temp_dir();
        setup_hooks(&dir).unwrap();

        // Check hook scripts were written to .loopdeck/hooks/
        let hooks_dir = dir.join(".loopdeck").join("hooks");

        let stop_py = hooks_dir.join("loopdeck-stop-hook.py");
        assert!(stop_py.exists());
        let stop_content = fs::read_to_string(&stop_py).unwrap();
        assert!(stop_content.contains("LoopDeck Stop hook"));

        let mem_sh = hooks_dir.join("loopdeck-memory-write.sh");
        assert!(mem_sh.exists());
        let mem_content = fs::read_to_string(&mem_sh).unwrap();
        assert!(mem_content.contains("LoopDeck memory auto-write"));

        let orch_py = hooks_dir.join("orchestrator-start.py");
        assert!(orch_py.exists());
        let orch_content = fs::read_to_string(&orch_py).unwrap();
        assert!(orch_content.contains("orchestrator-start hook"));

        let cap_py = hooks_dir.join("loopdeck-decisions-cap.py");
        assert!(cap_py.exists());
        let cap_content = fs::read_to_string(&cap_py).unwrap();
        assert!(cap_content.contains("decisions.md live-entry cap"));

        let dirty_py = hooks_dir.join("loopdeck-dirty-flag.py");
        assert!(dirty_py.exists());
        let dirty_content = fs::read_to_string(&dirty_py).unwrap();
        assert!(dirty_content.contains("marks the session as dirty"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_setup_hooks_script_idempotent() {
        let dir = temp_dir();

        // First call writes scripts
        setup_hooks(&dir).unwrap();

        // Modify a script to simulate user customization
        let stop_py = dir
            .join(".loopdeck")
            .join("hooks")
            .join("loopdeck-stop-hook.py");
        let custom = "# Customized by user\nprint('hello')\n";
        fs::write(&stop_py, custom).unwrap();

        // Second call should NOT overwrite the customized script
        setup_hooks(&dir).unwrap();
        let content = fs::read_to_string(&stop_py).unwrap();
        assert_eq!(content, custom);

        fs::remove_dir_all(&dir).unwrap();
    }
}
