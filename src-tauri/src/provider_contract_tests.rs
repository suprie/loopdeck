//! Fixture-based contract tests for both provider adapters.
//!
//! These tests load the sanitized wire fragments under
//! `tests/fixtures/provider-events/` and assert the *semantic* output of
//! `provider_normalize`, not wire syntax: equivalent behavior from Claude and
//! Codex must produce the same payload variants, and every declared terminal
//! must be exactly one durable terminal per segment.
//!
//! Covered regressions from `prd-provider-contract` Phase 3:
//! - one terminal per run segment, including when the provider terminal line
//!   is duplicated;
//! - approval/question/plan payloads survive normalization (`detail_redacted`
//!   is not dropped), so the migrated frontend consumer can render the card;
//! - automatic policy approval is `allowed` + `auto-allow:` summary, never a
//!   separate enum variant;
//! - Codex capability gaps are explicit (`plan_approval` absent, an attempted
//!   plan request becomes `unsupported`).

use crate::provider_event::{
    Capability, DecisionState, Provider, RunEvent, RunEventPayload, TerminalOutcome,
};
use crate::provider_normalize::{RunEventNormalizer, AUTO_ALLOW_PREFIX};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Manifest {
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

fn normalize_claude(scenario: &Scenario) -> (RunEventNormalizer, Vec<RunEvent>) {
    let text = fs::read_to_string(fixture_root().join(&scenario.claude))
        .unwrap_or_else(|e| panic!("read Claude fixture {}: {e}", scenario.claude));
    let mut normalizer = RunEventNormalizer::new("run-claude-fixture", Provider::Claude);
    let mut events = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        events.extend(normalizer.convert_claude_line(line));
    }
    (normalizer, events)
}

fn normalize_codex(scenario: &Scenario) -> (RunEventNormalizer, Vec<RunEvent>) {
    let text = fs::read_to_string(fixture_root().join(&scenario.codex))
        .unwrap_or_else(|e| panic!("read Codex fixture {}: {e}", scenario.codex));
    let mut normalizer = RunEventNormalizer::new("run-codex-fixture", Provider::Codex);
    let mut events = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let message: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("parse Codex fixture line: {e}\n{line}"));
        events.extend(normalizer.convert_codex_message(&message));
    }
    (normalizer, events)
}

fn variant(payload: &RunEventPayload) -> &'static str {
    match payload {
        RunEventPayload::Started { .. } => "started",
        RunEventPayload::Content { .. } => "content",
        RunEventPayload::Tool { .. } => "tool",
        RunEventPayload::Decision { .. } => "decision",
        RunEventPayload::Usage { .. } => "usage",
        RunEventPayload::Retrying { .. } => "retrying",
        RunEventPayload::Terminal { .. } => "terminal",
        RunEventPayload::Unsupported { .. } => "unsupported",
    }
}

fn of_kind<'a>(events: &'a [RunEvent], kind: &str) -> Vec<&'a RunEvent> {
    events
        .iter()
        .filter(|event| variant(&event.payload) == kind)
        .collect()
}

fn terminal_outcomes(events: &[RunEvent]) -> Vec<TerminalOutcome> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            RunEventPayload::Terminal { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .collect()
}

fn concatenated_text(events: &[RunEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            RunEventPayload::Content { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Compare meaning, not wire chunking: providers split text into different
/// blocks/deltas and may include the inter-sentence space in the next chunk.
fn semantic_words(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric() && ch != '\'')
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn expected_outcome(name: &str) -> TerminalOutcome {
    match name {
        "completed" => TerminalOutcome::Completed,
        "error" => TerminalOutcome::Error,
        "interrupted" => TerminalOutcome::Interrupted,
        "cancelled" => TerminalOutcome::Cancelled,
        "killed" => TerminalOutcome::Killed,
        other => panic!("unknown terminal outcome {other}"),
    }
}

#[test]
fn paired_fixtures_cover_declared_payloads_for_both_providers() {
    for scenario in load_manifest().scenarios {
        let (_, claude) = normalize_claude(&scenario);
        let (_, codex) = normalize_codex(&scenario);

        for payload in &scenario.expected_payloads {
            assert!(
                !of_kind(&claude, payload).is_empty(),
                "scenario \"{}\" (claude) produced no {payload}",
                scenario.id
            );
            assert!(
                !of_kind(&codex, payload).is_empty(),
                "scenario \"{}\" (codex) produced no {payload}",
                scenario.id
            );
        }
    }
}

#[test]
fn content_fixtures_normalize_to_the_same_text() {
    for scenario in load_manifest().scenarios {
        if !scenario.expected_payloads.iter().any(|p| p == "content") {
            continue;
        }
        let (_, claude) = normalize_claude(&scenario);
        let (_, codex) = normalize_codex(&scenario);
        assert_eq!(
            semantic_words(&concatenated_text(&claude)),
            semantic_words(&concatenated_text(&codex)),
            "scenario \"{}\" text differs between providers",
            scenario.id
        );
        assert!(
            !concatenated_text(&claude).is_empty(),
            "scenario \"{}\" produced no text",
            scenario.id
        );
    }
}

#[test]
fn tool_fixtures_preserve_redacted_input() {
    for scenario in load_manifest().scenarios {
        if !scenario.expected_payloads.iter().any(|p| p == "tool") {
            continue;
        }
        for (provider, events) in [
            ("claude", normalize_claude(&scenario).1),
            ("codex", normalize_codex(&scenario).1),
        ] {
            for event in of_kind(&events, "tool") {
                match &event.payload {
                    RunEventPayload::Tool {
                        name,
                        input_redacted,
                    } => {
                        assert!(!name.is_empty(), "{provider} tool has no name");
                        assert!(
                            !input_redacted.is_empty(),
                            "scenario \"{}\" ({provider}) lost the tool input",
                            scenario.id
                        );
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[test]
fn decision_fixtures_preserve_kind_state_and_render_payload() {
    for scenario in load_manifest().scenarios {
        if !scenario.expected_payloads.iter().any(|p| p == "decision") {
            continue;
        }
        let (_, claude) = normalize_claude(&scenario);
        let (_, codex) = normalize_codex(&scenario);

        let kinds = |events: &[RunEvent]| {
            let mut kinds = of_kind(events, "decision")
                .into_iter()
                .map(|event| match &event.payload {
                    RunEventPayload::Decision { kind, .. } => format!("{kind:?}"),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            kinds.sort();
            kinds
        };
        assert_eq!(
            kinds(&claude),
            kinds(&codex),
            "scenario \"{}\" decision kinds differ",
            scenario.id
        );

        for event in of_kind(&claude, "decision")
            .into_iter()
            .chain(of_kind(&codex, "decision"))
        {
            match &event.payload {
                RunEventPayload::Decision {
                    state,
                    detail_redacted,
                    ..
                } => {
                    assert_eq!(*state, DecisionState::Pending);
                    let detail = detail_redacted
                        .as_deref()
                        .expect("decision render payload must not be lost");
                    assert!(
                        serde_json::from_str::<Value>(detail).is_ok(),
                        "decision detail is not structured JSON: {detail}"
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn approval_fixtures_keep_the_command_and_tool_name() {
    let scenario = load_manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == "decisions")
        .expect("decisions scenario");

    for (provider, events) in [
        ("claude", normalize_claude(&scenario).1),
        ("codex", normalize_codex(&scenario).1),
    ] {
        let approval = of_kind(&events, "decision")
            .into_iter()
            .find(|event| {
                matches!(
                    &event.payload,
                    RunEventPayload::Decision {
                        kind: crate::provider_event::DecisionKind::Approval,
                        ..
                    }
                )
            })
            .unwrap_or_else(|| panic!("{provider} fixture produced no approval decision"));
        let RunEventPayload::Decision {
            detail_redacted, ..
        } = &approval.payload
        else {
            unreachable!()
        };
        let detail: Value =
            serde_json::from_str(detail_redacted.as_deref().expect("approval detail")).unwrap();
        assert_eq!(
            detail.get("tool_name").and_then(Value::as_str),
            Some("Bash"),
            "{provider} approval lost its tool name"
        );
        assert!(
            detail
                .get("input")
                .and_then(|input| input.get("command"))
                .and_then(Value::as_str)
                .is_some_and(|command| command.contains("cargo test")),
            "{provider} approval lost its command payload: {detail}"
        );
    }
}

#[test]
fn question_fixtures_keep_the_structured_questions() {
    let scenario = load_manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == "decisions")
        .expect("decisions scenario");

    for (provider, events) in [
        ("claude", normalize_claude(&scenario).1),
        ("codex", normalize_codex(&scenario).1),
    ] {
        let question = of_kind(&events, "decision")
            .into_iter()
            .find(|event| {
                matches!(
                    &event.payload,
                    RunEventPayload::Decision {
                        kind: crate::provider_event::DecisionKind::Question,
                        ..
                    }
                )
            })
            .unwrap_or_else(|| panic!("{provider} fixture produced no question decision"));
        let RunEventPayload::Decision {
            detail_redacted, ..
        } = &question.payload
        else {
            unreachable!()
        };
        let questions: Value =
            serde_json::from_str(detail_redacted.as_deref().expect("question detail")).unwrap();
        assert_eq!(
            questions
                .get(0)
                .and_then(|question| question.get("question"))
                .and_then(Value::as_str),
            Some("Which scope should this change cover?"),
            "{provider} question payload lost its question text"
        );
        assert!(
            questions
                .get(0)
                .and_then(|question| question.get("options"))
                .and_then(Value::as_array)
                .is_some_and(|options| !options.is_empty()),
            "{provider} question payload lost its options"
        );
    }
}

#[test]
fn usage_fixtures_normalize_to_the_same_token_counts() {
    for scenario in load_manifest().scenarios {
        if !scenario.expected_payloads.iter().any(|p| p == "usage") {
            continue;
        }
        let counts = |events: &[RunEvent]| {
            of_kind(events, "usage")
                .into_iter()
                .map(|event| match &event.payload {
                    RunEventPayload::Usage {
                        input_tokens,
                        output_tokens,
                        ..
                    } => (*input_tokens, *output_tokens),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
        };
        let claude = counts(&normalize_claude(&scenario).1);
        let codex = counts(&normalize_codex(&scenario).1);
        assert_eq!(
            claude, codex,
            "scenario \"{}\" usage differs between providers",
            scenario.id
        );
    }
}

#[test]
fn every_terminal_scenario_emits_exactly_one_terminal_per_provider() {
    for scenario in load_manifest().scenarios {
        let (_, claude) = normalize_claude(&scenario);
        let (_, codex) = normalize_codex(&scenario);
        match &scenario.terminal {
            Some(name) => {
                let expected = expected_outcome(name);
                for (provider, events) in [("claude", &claude), ("codex", &codex)] {
                    let outcomes = terminal_outcomes(events);
                    assert_eq!(
                        outcomes,
                        vec![expected.clone()],
                        "scenario \"{}\" ({provider}) terminal mismatch",
                        scenario.id
                    );
                }
            }
            None => {
                assert!(
                    terminal_outcomes(&claude).is_empty(),
                    "scenario \"{}\" (claude) fragment has a terminal",
                    scenario.id
                );
                assert!(
                    terminal_outcomes(&codex).is_empty(),
                    "scenario \"{}\" (codex) fragment has a terminal",
                    scenario.id
                );
            }
        }
    }
}

#[test]
fn duplicated_provider_terminal_does_not_create_a_second_terminal() {
    for scenario in load_manifest()
        .scenarios
        .into_iter()
        .filter(|scenario| scenario.terminal.is_some())
    {
        // Claude: replay the last line (the provider's terminal line).
        let text =
            fs::read_to_string(fixture_root().join(&scenario.claude)).expect("Claude fixture");
        let last_line = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .expect("non-empty fixture");
        let mut normalizer = RunEventNormalizer::new("run-claude-dup", Provider::Claude);
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let _ = normalizer.convert_claude_line(line);
        }
        let duplicate = normalizer.convert_claude_line(last_line);
        assert!(
            !duplicate
                .iter()
                .any(|event| variant(&event.payload) == "terminal"),
            "scenario \"{}\" (claude) emitted a duplicate terminal",
            scenario.id
        );

        // Codex: replay the `turn/completed` message.
        let text = fs::read_to_string(fixture_root().join(&scenario.codex)).expect("Codex fixture");
        let last_message: Value = serde_json::from_str(
            text.lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .expect("non-empty fixture"),
        )
        .expect("Codex terminal line is JSON");
        let mut normalizer = RunEventNormalizer::new("run-codex-dup", Provider::Codex);
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let message: Value = serde_json::from_str(line).expect("Codex line");
            let _ = normalizer.convert_codex_message(&message);
        }
        let duplicate = normalizer.convert_codex_message(&last_message);
        assert!(
            !duplicate
                .iter()
                .any(|event| variant(&event.payload) == "terminal"),
            "scenario \"{}\" (codex) emitted a duplicate terminal",
            scenario.id
        );
    }
}

#[test]
fn started_events_carry_provider_capabilities_and_codex_omits_plan_approval() {
    let scenario = load_manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == "completed")
        .expect("completed scenario");

    let (_, claude_events) = normalize_claude(&scenario);
    let (_, codex_events) = normalize_codex(&scenario);
    let claude_started = of_kind(&claude_events, "started")
        .into_iter()
        .next()
        .expect("Claude started event");
    let codex_started = of_kind(&codex_events, "started")
        .into_iter()
        .next()
        .expect("Codex started event");

    let claude_caps = match &claude_started.payload {
        RunEventPayload::Started { capabilities, .. } => capabilities,
        _ => unreachable!(),
    };
    let codex_caps = match &codex_started.payload {
        RunEventPayload::Started { capabilities, .. } => capabilities,
        _ => unreachable!(),
    };
    assert!(claude_caps.supported.contains(&Capability::PlanApproval));
    assert!(!codex_caps.supported.contains(&Capability::PlanApproval));
}

#[test]
fn automatic_approval_is_allowed_with_auto_allow_summary() {
    let mut normalizer = RunEventNormalizer::new("run-auto", Provider::Claude);
    let event = normalizer.approval_resolved(
        "req-auto",
        "Bash",
        &serde_json::json!({"command": "cargo test"}),
        DecisionState::Allowed,
        "",
        true,
    );
    match event.payload {
        RunEventPayload::Decision { state, summary, .. } => {
            assert_eq!(state, DecisionState::Allowed);
            assert!(summary.expect("summary").starts_with(AUTO_ALLOW_PREFIX));
        }
        other => panic!("expected approval decision, got {other:?}"),
    }
}
