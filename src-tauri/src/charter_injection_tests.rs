//! Per-path charter-injection integration tests (`prd-role-foundations`
//! Phase 2, loop `role-foundations/injection-tests`).
//!
//! Strategy: fake child binaries. Fixture `claude` / `codex` executables are
//! planted once per test process and wired in through the
//! `LOOPDECK_CLAUDE_BIN` / `LOOPDECK_CODEX_BIN` overrides (`binary::env_override`),
//! and every test spawns through the REAL production spawn code — no mocks of
//! `HarnessSession::spawn` or the adapters. The fakes dump their argv (and,
//! for Codex, the JSON-RPC stdin lines) into the child's working directory,
//! which each test makes unique, so parallel tests cannot observe each other's
//! captures and no process-global env mutation is needed beyond the identical
//! bin-dir override.
//!
//! The three paths under test:
//!
//! 1. **Interactive** — `commands::state::with_session` (the send/composer
//!    path): resolves the default roster entry, spawns, caches.
//! 2. **Run-queue** — `commands::agent::spawn_fresh` (the fresh-session
//!    primitive `start_fresh_and_record_streaming_in_root` drives for every
//!    queued phase): same resolver, forced-autonomous policy.
//! 3. **Multi-agent** — `resolve_agent_config_by_id` +
//!    `commands::agent::spawn_fresh_with_config` (the exact pair
//!    `multi_agent::execute_subrun` uses with a profile resolved before
//!    worktrees are spawned).

use crate::claude_session::{
    InterruptSlot, ParkSlots, PermissionSlot, PlanSlot, QuestionSlot,
};
use crate::commands::agent::{spawn_fresh, spawn_fresh_with_config};
use crate::commands::state::with_session;
use crate::commands::state::AppState;
use crate::config::{AgentConfig, AgentHarness, GlobalConfig, NamedAgentConfig, RoleCharter};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

// ── Fixtures ────────────────────────────────────────────────────────────────

/// The charter every injection test asserts against. Distinctive strings so a
/// capture file can never match by accident.
fn qa_charter() -> RoleCharter {
    RoleCharter {
        persona: Some("You are the QA-role agent. You verify work; you do not build.".into()),
        allowed_skills: vec!["loopdeck-prd-verifier".into()],
        output_contract: Some(
            "End every final message with a Verdict line: PASS, WARN, or BLOCK.".into(),
        ),
        rules: None,
    }
}

fn chartered_claude_config() -> AgentConfig {
    AgentConfig {
        charter: Some(qa_charter()),
        ..AgentConfig::default()
    }
}

/// The fake `claude`: one argv line per argument into the cwd, then drain
/// stdin forever so the parent's writes never block. Spawn code sets the
/// child's cwd to the project path, so each test reads its own capture.
const FAKE_CLAUDE: &str = concat!(
    "#!/bin/sh\n",
    "printf '%s\\n' \"$@\" > claude-argv.txt\n",
    "exec cat > /dev/null\n",
);

/// The fake `codex` app-server: argv capture, then a minimal JSON-RPC loop —
/// echo every stdin line into `codex-stdin.jsonl` and answer the handshake
/// methods (`initialize`, `thread/start`) so production code reaches the
/// `turn/start` write, whose first input item is where the charter is
/// prepended. `turn/start` itself is never answered: the tests assert the
/// captured stdin line and then drop the session.
const FAKE_CODEX: &str = concat!(
    "#!/bin/sh\n",
    "printf '%s\\n' \"$@\" > codex-argv.txt\n",
    "while IFS= read -r line; do\n",
    "  printf '%s\\n' \"$line\" >> codex-stdin.jsonl\n",
    "  id=$(printf '%s' \"$line\" | sed -n 's/.*\"id\":\\([0-9][0-9]*\\).*/\\1/p' | head -n 1)\n",
    "  method=$(printf '%s' \"$line\" | sed -n 's/.*\"method\":\"\\([^\"]*\\)\".*/\\1/p' | head -n 1)\n",
    "  case \"$method\" in\n",
    "    initialize) printf '{\"id\":%s,\"result\":{}}\\n' \"$id\" ;;\n",
    "    thread/start|thread/resume) printf '{\"id\":%s,\"result\":{\"thread\":{\"id\":\"fake-thread\"}}}\\n' \"$id\" ;;\n",
    "  esac\n",
    "done\n",
);

/// The fake Code Mode host sidecar: publish a loopback endpoint, then idle.
/// `spawn_code_mode_host` requires the sibling binary before the app-server
/// is even started, so the fixture directory must provide it.
const FAKE_CODE_MODE_HOST: &str = concat!(
    "#!/bin/sh\n",
    "printf 'ws://127.0.0.1:1\\n'\n",
    "exec sleep 300\n",
);

fn write_exec(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fake binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake binary");
    }
}

/// Plant the fake harness binaries once per test process and point the
/// `binary` module's env overrides at them. The override is checked before
/// the process-wide PATH cache, so no test can be poisoned by the machine's
/// real `claude`/`codex`.
fn fake_harness_bin_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-fake-harness-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create fake bin dir");
        write_exec(&dir, "claude", FAKE_CLAUDE);
        write_exec(&dir, "codex", FAKE_CODEX);
        write_exec(&dir, "codex-code-mode-host", FAKE_CODE_MODE_HOST);
        // Same value for every test → no cross-test interference despite env
        // being process-global.
        std::env::set_var("LOOPDECK_CLAUDE_BIN", dir.join("claude"));
        std::env::set_var("LOOPDECK_CODEX_BIN", dir.join("codex"));
        dir
    })
}

/// A unique per-test project directory. Uniqueness is what keeps the fakes'
/// cwd-relative capture files collision-free under the parallel test runner.
fn fresh_project_dir(label: &str) -> PathBuf {
    fake_harness_bin_dir();
    let dir = std::env::temp_dir().join(format!(
        "loopdeck-charter-{}-{}",
        label,
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).expect("create project dir");
    dir
}

fn empty_state() -> AppState {
    AppState {
        config: Mutex::new(GlobalConfig::default()),
        claude_sessions: Mutex::new(HashMap::new()),
        pending_answers: Mutex::new(HashMap::new()),
        pending_permissions: Mutex::new(HashMap::new()),
        pending_plans: Mutex::new(HashMap::new()),
        interrupt_slots: Mutex::new(HashMap::new()),
        run_handles: Mutex::new(HashMap::new()),
        multi_agent_active_runs: Mutex::new(HashSet::new()),
        multi_agent_manifest_locks: Mutex::new(HashMap::new()),
    }
}

/// An `AppState` whose default roster entry carries the QA charter — the
/// shape `resolve_agent_config` sees on the interactive and run-queue paths.
fn state_with_chartered_default() -> AppState {
    let agent = NamedAgentConfig::new("QA".into(), chartered_claude_config())
        .expect("valid named agent");
    let state = empty_state();
    {
        let mut config = state.config.lock().unwrap();
        config.default_agent_id = Some(agent.id.clone());
        config.agents.push(agent);
    }
    state
}

/// Read a capture file produced by a fake child, retrying briefly: the fake
/// writes asynchronously right after exec, so the read can legitimately lose
/// the race with `spawn()` returning.
fn read_capture(path: &Path) -> String {
    for _ in 0..100 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if !contents.is_empty() {
                return contents;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("capture file not written within 5s: {}", path.display());
}

/// Assert the argv capture shows the full charter riding
/// `--append-system-prompt`, rendered as the one shared charter block.
fn assert_claude_charter_argv(project_dir: &Path) {
    let capture = read_capture(&project_dir.join("claude-argv.txt"));
    // The rendered charter is one argv value containing newlines, so slice
    // the capture after the flag rather than indexing per-line argv.
    assert_eq!(
        capture.matches("--append-system-prompt").count(),
        1,
        "expected exactly one --append-system-prompt, argv:\n{capture}"
    );
    let rendered = capture
        .split_once("--append-system-prompt\n")
        .expect("flag present")
        .1;
    for expected in [
        "# Role charter",
        "## Persona",
        "You are the QA-role agent",
        "## Allowed skills",
        "- loopdeck-prd-verifier",
        "## Output contract",
    ] {
        assert!(
            rendered.contains(expected),
            "charter block missing {expected:?}:\n{rendered}"
        );
    }
}

// ── Path 1: interactive (with_session) ─────────────────────────────────────

#[tokio::test]
async fn interactive_path_injects_charter_into_claude_argv() {
    let state = state_with_chartered_default();
    let project = fresh_project_dir("interactive");

    let session = with_session(&state, &project)
        .await
        .expect("interactive session should spawn");

    assert_eq!(session.lock().await.harness(), AgentHarness::Claude);
    assert_claude_charter_argv(&project);
}

// ── Path 2: run queue (spawn_fresh, forced autonomous) ─────────────────────

#[tokio::test]
async fn run_queue_path_injects_charter_into_claude_argv() {
    let state = state_with_chartered_default();
    let project = fresh_project_dir("run-queue");

    // `force_autonomous: true` mirrors the run-queue executor's spawn.
    let session = spawn_fresh(&state, &project, &project, true)
        .await
        .expect("run-queue session should spawn");

    assert_eq!(session.lock().await.harness(), AgentHarness::Claude);
    assert_claude_charter_argv(&project);
}

// ── Path 3: multi-agent (resolved-by-id config + spawn_fresh_with_config) ──

#[tokio::test]
async fn multi_agent_path_injects_charter_into_claude_argv() {
    let state = state_with_chartered_default();
    let agent_id = state
        .config
        .lock()
        .unwrap()
        .default_agent_id
        .clone()
        .expect("default agent id");

    // The exact resolution multi-agent runs use before spawning worktrees.
    let config =
        crate::commands::state::resolve_agent_config_by_id(&state, &agent_id).expect("resolve");
    assert!(
        config.charter.is_some(),
        "resolve_agent_config_by_id must preserve the charter"
    );

    let worktree = fresh_project_dir("multi-agent");
    let session = spawn_fresh_with_config(&state, &worktree, &worktree, &config, true)
        .expect("multi-agent session should spawn");

    assert_eq!(session.lock().await.harness(), AgentHarness::Claude);
    assert_claude_charter_argv(&worktree);
}

// ── Negative: no charter → no injection ─────────────────────────────────────

#[tokio::test]
async fn charterless_agent_spawns_without_system_prompt_flag() {
    let state = empty_state();
    let project = fresh_project_dir("charterless");
    let config = AgentConfig::default();

    let session = spawn_fresh_with_config(&state, &project, &project, &config, false)
        .expect("charterless session should spawn");

    assert_eq!(session.lock().await.harness(), AgentHarness::Claude);
    let capture = read_capture(&project.join("claude-argv.txt"));
    assert!(
        !capture.contains("--append-system-prompt"),
        "charterless spawn must not inject, argv:\n{capture}"
    );
}

// ── Codex: charter prepended to the first task prompt ───────────────────────

#[tokio::test]
async fn codex_path_prepends_charter_to_first_task_prompt() {
    let state = empty_state();
    let project = fresh_project_dir("codex");
    let config = AgentConfig {
        harness: AgentHarness::Codex,
        charter: Some(qa_charter()),
        ..AgentConfig::default()
    };

    let session = spawn_fresh_with_config(&state, &project, &project, &config, true)
        .expect("codex session should spawn");
    assert_eq!(session.lock().await.harness(), AgentHarness::Codex);

    // The fake app-server never answers turn/start, so drive the send in a
    // task, wait for the captured stdin line to appear, assert, and abort.
    let sender = tokio::spawn(async move {
        let mut guard = session.lock().await;
        let qslot = QuestionSlot::default();
        let pslot = PermissionSlot::default();
        let plnslot = PlanSlot::default();
        let islot = InterruptSlot::default();
        let slots = ParkSlots {
            question: &qslot,
            permission: &pslot,
            plan: &plnslot,
        };
        let _ = guard
            .send_message("FIX THE BUG NOW", &[], &slots, &islot)
            .await;
    });

    let stdin_path = project.join("codex-stdin.jsonl");
    let turn_line = loop {
        if let Ok(contents) = std::fs::read_to_string(&stdin_path) {
            if let Some(line) = contents
                .lines()
                .rev()
                .find(|l| l.contains("\"method\":\"turn/start\""))
            {
                break line.to_string();
            }
        }
        assert!(
            !sender.is_finished(),
            "send task ended before turn/start was captured"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    // Role identity first, task second: the charter block must appear inside
    // the first input item, ahead of the task text.
    let charter_pos = turn_line
        .find("# Role charter")
        .expect("turn/start input must carry the charter block");
    let task_pos = turn_line
        .find("FIX THE BUG NOW")
        .expect("turn/start input must carry the task text");
    assert!(
        charter_pos < task_pos,
        "charter must precede the task prompt:\n{turn_line}"
    );
    assert!(
        turn_line.contains("You are the QA-role agent"),
        "charter persona must be rendered:\n{turn_line}"
    );
    // Sanity: the child really was the app-server fixture.
    let argv = read_capture(&project.join("codex-argv.txt"));
    assert!(argv.contains("app-server"), "argv:\n{argv}");

    sender.abort();
}
