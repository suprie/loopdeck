//! Integrity checks for the sanitized provider wire fixtures under
//! `tests/fixtures/provider-events/`.
//!
//! These fixtures are the input to the provider-normalization contract tests
//! (`prd-provider-contract` Phase 3). Keeping the capture step honest here —
//! parseable wire shapes, declared semantic coverage, exactly one terminal
//! event per terminating fragment, and no unsanitized values — prevents the
//! later conversion tests from passing against fixtures that no longer
//! resemble either provider.

use crate::provider_event::TerminalOutcome;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const PROVIDERS: [&str; 2] = ["claude", "codex"];

/// Payload names that may appear in `manifest.json`. Mirrors the
/// `RunEventPayload` variants in `provider_event.rs`.
const KNOWN_PAYLOADS: [&str; 8] = [
    "started",
    "content",
    "tool",
    "decision",
    "usage",
    "retrying",
    "terminal",
    "unsupported",
];

#[derive(Debug, Deserialize)]
struct Manifest {
    version: u16,
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    id: String,
    claude: String,
    codex: String,
    expected_payloads: Vec<String>,
    #[serde(default)]
    terminal: Option<String>,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider-events")
}

fn load_manifest() -> Manifest {
    let path = fixture_root().join("manifest.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn read_fixture(relative: &str) -> String {
    let path = fixture_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn non_empty_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.trim().is_empty())
}

fn jsonl_files(provider: &str) -> Vec<PathBuf> {
    let dir = fixture_root().join(provider);
    let mut files = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("fixture dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

/// One anchor per semantic payload that must appear in a provider fragment.
/// Anchors are wire substrings, not parsed values, because the conversion
/// layer that will parse them is deliberately deferred to Phase 2.
fn payload_anchors(provider: &str, payload: &str) -> &'static [&'static str] {
    match (provider, payload) {
        ("claude", "started") => &["\"type\":\"system\""],
        ("claude", "content") => &["\"type\":\"text\""],
        ("claude", "tool") => &["\"type\":\"tool_use\""],
        ("claude", "decision") => &["\"type\":\"control_request\""],
        ("claude", "usage") => &["\"usage\":"],
        ("claude", "terminal") => &["\"type\":\"result\""],
        ("codex", "started") => &["\"method\":\"turn/started\""],
        ("codex", "content") => &["\"method\":\"item/agentMessage/delta\""],
        ("codex", "tool") => &["\"type\":\"commandExecution\"", "\"type\":\"fileChange\""],
        ("codex", "decision") => &["requestUserInput", "requestApproval"],
        ("codex", "usage") => &["thread/tokenUsage/updated"],
        ("codex", "terminal") => &["\"method\":\"turn/completed\""],
        _ => &[],
    }
}

/// Count terminal events in one provider fragment.
fn terminal_count(relative: &str, provider: &str) -> usize {
    let text = read_fixture(relative);
    non_empty_lines(&text)
        .filter(|line| {
            let event: Value = serde_json::from_str(line).expect("fixture line is JSON");
            match provider {
                "claude" => event.get("type").and_then(Value::as_str) == Some("result"),
                "codex" => event.get("method").and_then(Value::as_str) == Some("turn/completed"),
                _ => false,
            }
        })
        .count()
}

#[test]
fn manifest_references_every_fixture_and_declares_known_payloads() {
    let manifest = load_manifest();
    assert_eq!(manifest.version, 1, "unexpected manifest version");

    let mut ids = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for scenario in &manifest.scenarios {
        assert!(ids.insert(scenario.id.clone()), "duplicate scenario id");
        assert!(
            !scenario.expected_payloads.is_empty(),
            "scenario \"{}\" declares no payloads",
            scenario.id
        );
        for payload in &scenario.expected_payloads {
            assert!(
                KNOWN_PAYLOADS.contains(&payload.as_str()),
                "scenario \"{}\" declares unknown payload \"{payload}\"",
                scenario.id
            );
        }
        if let Some(terminal) = &scenario.terminal {
            serde_json::from_value::<TerminalOutcome>(Value::String(terminal.clone()))
                .unwrap_or_else(|_| panic!("unknown terminal outcome \"{terminal}\""));
        }
        for relative in [&scenario.claude, &scenario.codex] {
            assert!(
                fixture_root().join(relative).is_file(),
                "scenario \"{}\" references missing fixture {relative}",
                scenario.id
            );
            assert!(
                referenced.insert(relative.clone()),
                "fixture {relative} is referenced by more than one scenario"
            );
        }
    }

    for provider in PROVIDERS {
        for path in jsonl_files(provider) {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("fixture file name");
            let relative = format!("{provider}/{name}");
            assert!(
                referenced.contains(&relative),
                "fixture {relative} is not declared in manifest.json"
            );
        }
    }
}

#[test]
fn every_claude_line_parses_through_the_stream_parser() {
    for path in jsonl_files("claude") {
        let text = fs::read_to_string(&path).expect("read Claude fixture");
        assert!(
            non_empty_lines(&text).next().is_some(),
            "{} is empty",
            path.display()
        );
        for (index, line) in non_empty_lines(&text).enumerate() {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|e| panic!("{}:{} is not JSON: {e}", path.display(), index + 1));
            assert!(
                crate::agents::parse_stream_line(line).is_some(),
                "{}:{} is not recognized by parse_stream_line: {line}",
                path.display(),
                index + 1
            );
        }
    }
}

#[test]
fn every_codex_line_is_a_method_envelope() {
    for path in jsonl_files("codex") {
        let text = fs::read_to_string(&path).expect("read Codex fixture");
        assert!(
            non_empty_lines(&text).next().is_some(),
            "{} is empty",
            path.display()
        );
        for (index, line) in non_empty_lines(&text).enumerate() {
            let event: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{}:{} is not JSON: {e}", path.display(), index + 1));
            let method = event
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{}:{} has no method", path.display(), index + 1));
            assert!(
                !method.is_empty(),
                "{}:{} has an empty method",
                path.display(),
                index + 1
            );
            assert!(
                event.get("params").is_some_and(Value::is_object),
                "{}:{} has no params object",
                path.display(),
                index + 1
            );
        }
    }
}

#[test]
fn scenarios_cover_declared_semantic_payloads() {
    for scenario in &load_manifest().scenarios {
        for (provider, relative) in [("claude", &scenario.claude), ("codex", &scenario.codex)] {
            let text = read_fixture(relative);
            for payload in &scenario.expected_payloads {
                let anchors = payload_anchors(provider, payload);
                assert!(
                    !anchors.is_empty(),
                    "no anchors defined for {provider}/{payload}"
                );
                assert!(
                    anchors.iter().any(|anchor| text.contains(anchor)),
                    "scenario \"{}\" ({provider}) does not cover payload \"{payload}\"",
                    scenario.id
                );
            }
        }
    }
}

#[test]
fn fragments_have_exactly_one_terminal_event_when_declared() {
    for scenario in &load_manifest().scenarios {
        let claude = terminal_count(&scenario.claude, "claude");
        let codex = terminal_count(&scenario.codex, "codex");
        match &scenario.terminal {
            Some(expected) => {
                assert_eq!(
                    claude, 1,
                    "scenario \"{}\" (claude) must contain exactly one terminal event",
                    scenario.id
                );
                assert_eq!(
                    codex, 1,
                    "scenario \"{}\" (codex) must contain exactly one terminal event",
                    scenario.id
                );
                let expected_status = match expected.as_str() {
                    "completed" => "completed",
                    "error" => "failed",
                    "interrupted" => "interrupted",
                    "cancelled" => "cancelled",
                    "killed" => "killed",
                    other => panic!("unhandled terminal outcome \"{other}\""),
                };
                let text = read_fixture(&scenario.codex);
                let completed = non_empty_lines(&text)
                    .map(|line| serde_json::from_str::<Value>(line).expect("fixture line"))
                    .find(|event| {
                        event.get("method").and_then(Value::as_str) == Some("turn/completed")
                    })
                    .expect("declared Codex terminal event");
                assert_eq!(
                    completed
                        .pointer("/params/turn/status")
                        .and_then(Value::as_str),
                    Some(expected_status),
                    "scenario \"{}\" (codex) declares {expected} but the wire status differs",
                    scenario.id
                );
            }
            None => {
                assert_eq!(
                    claude, 0,
                    "scenario \"{}\" (claude) is a fragment but contains a terminal event",
                    scenario.id
                );
                assert_eq!(
                    codex, 0,
                    "scenario \"{}\" (codex) is a fragment but contains a terminal event",
                    scenario.id
                );
            }
        }
    }
}

#[test]
fn fixtures_contain_no_secrets_or_home_paths() {
    for provider in PROVIDERS {
        for path in jsonl_files(provider) {
            let text = fs::read_to_string(&path).expect("read fixture");
            for needle in ["/Users/", "/home/", "C:\\Users\\"] {
                assert!(
                    !text.contains(needle),
                    "{} contains an unsanitized home path ({needle})",
                    path.display()
                );
            }
            if let Some((pattern, line)) = crate::secret_scan::find_secret(&text) {
                panic!(
                    "{} matches secret pattern \"{pattern}\": {line}",
                    path.display()
                );
            }
        }
    }
}
