//! Clean handoff record — `prd-verified-delivery-reconciliation.md` Phase 4,
//! loop `delivery-bookkeeping/clean-handoff`.
//!
//! Persisted once at delivery; consumed lazily. The record states that the
//! delivered branch + worktree are **retained for review** (PR stays
//! unmerged) and that the *next* run starts fresh from the repo's default
//! branch. Per the run's pre-answered clarification, nothing is created
//! eagerly at delivery: the `.loopdeck/runs/<next-branch>/` worktree comes
//! into existence only when the next loop starts (`commands::run_queue::
//! ensure_worktree` bases every new run branch on the default branch), and
//! the user is never switched off whatever they have checked out.

use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const HANDOFFS_DIR: &str = "handoffs";
const AGENT_ARTIFACTS_DIR: &str = "handoff-artifacts";
const MAX_FRONTMATTER_BYTES: usize = 1024;
const MAX_BODY_BYTES: usize = 8 * 1024;
const LEDGER_FILE: &str = "ledger.json";

/// Downstream injection cap per artifact (`prd-agent-handoff` Phase 2). The
/// store's body hard cap is the same number: an oversized artifact is a
/// producer bug, so injection never truncates it silently.
pub const MAX_INJECTION_BYTES: usize = MAX_BODY_BYTES;

/// Graph-native handoff audit data. Artifacts remain Markdown; the ledger is
/// deliberately plain JSON so nodes and edges can be queried independently.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HandoffLedger {
    #[serde(default)]
    pub nodes: Vec<HandoffLedgerNode>,
    #[serde(default)]
    pub links: Vec<HandoffLedgerLink>,
    /// One record per artifact handed from an upstream phase to a downstream
    /// phase. `run_id` scopes the entry to one run plan so the morning report
    /// can filter the project-wide ledger.
    #[serde(default)]
    pub handoffs: Vec<HandoffLedgerEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffLedgerNode {
    pub topic: String,
    pub path: String,
    pub author_role: String,
    pub phase: String,
    pub artifact_type: ArtifactType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffLedgerLink {
    pub from_topic: String,
    pub to_topic: String,
    pub relation: String,
}

/// One auditable handoff in the run report: which role produced the artifact,
/// which role consumed it, where it lives, and whether the consumer's
/// `## Handoff citations` block covered every heading and numbered item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffLedgerEntry {
    /// Run-plan id this handoff belongs to. Older ledgers without a run id
    /// stay readable (`None`) and never match a report filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub from_role: String,
    pub to_role: String,
    pub artifact_path: String,
    pub citation: CitationResult,
}

/// Coverage-only citation outcome (`prd-agent-handoff` Phase 3). The executor
/// checks that every section and item is mentioned or explicitly `not-used`;
/// it never judges fidelity or completeness of the consumer's wording.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CitationResult {
    /// Every heading and numbered item is covered.
    Pass,
    /// Some elements are covered, others are missing.
    Partial,
    /// No citation block, or no element is covered.
    Missing,
}

/// Per-element coverage parsed from a consumer's `## Handoff citations`
/// block. Kept for diagnostics; the ledger stores only the roll-up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CitationCoverage {
    pub cited: Vec<String>,
    pub not_used: Vec<String>,
    pub missing: Vec<String>,
}

impl CitationCoverage {
    pub fn result(&self) -> CitationResult {
        if self.missing.is_empty() {
            CitationResult::Pass
        } else if self.cited.is_empty() {
            CitationResult::Missing
        } else {
            CitationResult::Partial
        }
    }
}

/// The kinds of contract artifacts that can cross a phase boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Plan,
    Analysis,
    Decision,
    Report,
    Content,
}

/// Machine-readable metadata carried by an artifact's YAML frontmatter.
/// `path` is the durable path in the project's handoff store and is not
/// duplicated in frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub artifact: String,
    pub author_role: String,
    pub phase: String,
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    pub created: chrono::NaiveDate,
    pub summary: String,
    #[serde(default)]
    pub cites: Vec<String>,
    #[serde(skip)]
    pub path: PathBuf,
}

/// A validated handoff artifact, including the body that follows its
/// frontmatter. Artifacts are immutable once persisted: a duplicate topic is
/// an error so a retry can never overwrite the original handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub record: ArtifactRecord,
    pub body: String,
}

/// The single latest delivery handoff. Overwritten by each successful
/// delivery — it describes "where the last verified delivery landed and
/// where the next run starts from", not an audit log (that's
/// `execution.yaml` history + delivery links).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandoffRecord {
    /// Branch the delivery landed on (retained, PR unmerged).
    pub delivered_branch: String,
    /// Draft PR awaiting human review.
    pub pr_url: String,
    /// The retained managed worktree (`.loopdeck/runs/<branch>/`).
    pub worktree: PathBuf,
    /// Default branch the next run's branch is cut from.
    pub next_base: String,
    pub delivered_at: DateTime<Utc>,
}

pub fn handoff_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join("handoff.yaml")
}

/// The persistent, cross-run artifact store for this project.
pub fn store_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join(HANDOFFS_DIR)
}

pub fn ledger_path(repo_path: &Path) -> PathBuf {
    store_dir(repo_path).join(LEDGER_FILE)
}

pub fn load_ledger(repo_path: &Path) -> Result<HandoffLedger, AppError> {
    let path = ledger_path(repo_path);
    if !path.exists() {
        return Ok(HandoffLedger::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    serde_json::from_str(&raw)
        .map_err(|error| AppError::RunPlan(format!("handoff ledger is invalid JSON: {error}")))
}

pub fn save_ledger(repo_path: &Path, ledger: &HandoffLedger) -> Result<(), AppError> {
    let path = ledger_path(repo_path);
    let contents = serde_json::to_string_pretty(ledger).map_err(|error| {
        AppError::RunPlan(format!("could not serialize the handoff ledger: {error}"))
    })?;
    crate::persist::atomic_write(&path, &contents)?;
    Ok(())
}

/// Stable topic slug used by the executor when it asks an agent to emit a
/// phase artifact. The full execution id is retained, with separators mapped
/// to hyphens so it cannot escape the store directory.
pub fn topic_for_phase(phase: &str) -> String {
    phase
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn artifact_path(repo_path: &Path, topic: &str) -> Result<PathBuf, AppError> {
    let topic = topic_for_phase(topic);
    if topic.is_empty() {
        return Err(AppError::Conflict("handoff topic cannot be empty".into()));
    }
    Ok(store_dir(repo_path).join(format!("{topic}.md")))
}

/// Locate the agent-authored source in an isolated run worktree. The first
/// location is the executor's staging convention; the second is accepted for
/// agents that follow the public store path literally. Neither location is
/// ever used as the persistent destination in the main checkout.
pub fn agent_artifact_path(worktree: &Path, topic: &str) -> Result<PathBuf, AppError> {
    let topic = topic_for_phase(topic);
    if topic.is_empty() {
        return Err(AppError::Conflict("handoff topic cannot be empty".into()));
    }
    let staging = worktree
        .join(".loopdeck")
        .join(AGENT_ARTIFACTS_DIR)
        .join(format!("{topic}.md"));
    if staging.is_file() {
        return Ok(staging);
    }
    Ok(worktree
        .join(".loopdeck")
        .join(HANDOFFS_DIR)
        .join(format!("{topic}.md")))
}

pub fn load_artifact(repo_path: &Path, topic: &str) -> Result<Option<Artifact>, AppError> {
    let path = artifact_path(repo_path, topic)?;
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)?;
    Ok(Some(parse_artifact(
        &contents,
        &topic_for_phase(topic),
        path,
    )?))
}

/// Store-relative path a consumer cites and the run report records. Always
/// forward slashes so the citation form is stable across platforms.
pub fn store_relative_path(topic: &str) -> String {
    format!(".loopdeck/{HANDOFFS_DIR}/{}.md", topic_for_phase(topic))
}

/// Load one artifact for downstream injection. The body is bounded at the
/// contract's hard cap; an oversized artifact is an error, never truncated.
pub fn load_for_injection(repo_path: &Path, topic: &str) -> Result<Artifact, AppError> {
    let artifact = load_artifact(repo_path, topic)?.ok_or_else(|| {
        AppError::RunPlan(format!(
            "handoff artifact \"{}\" is missing from the store at {}",
            topic_for_phase(topic),
            store_relative_path(topic)
        ))
    })?;
    if artifact.body.len() > MAX_INJECTION_BYTES {
        return Err(AppError::RunPlan(format!(
            "handoff artifact \"{}\" exceeds the {MAX_INJECTION_BYTES}-byte injection cap",
            artifact.record.artifact
        )));
    }
    Ok(artifact)
}

/// One upstream artifact injected into a consumer phase's prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectedArtifact {
    pub topic: String,
    pub artifact: Artifact,
}

/// Render the bounded, contract-cited upstream block appended to a consumer
/// phase's prompt. Each artifact's body is already validated at
/// [`MAX_INJECTION_BYTES`]; the store path is always named so the consumer
/// can cite it.
pub fn render_injection(consumer_id: &str, artifacts: &[InjectedArtifact]) -> String {
    if artifacts.is_empty() {
        return String::new();
    }
    let mut block = format!(
        "\n\n## Upstream handoff artifacts for phase `{consumer_id}`\n\n\
         This phase depends on upstream phases that published contract \
         artifacts. Read each artifact below and cite it in your final message \
         using the contract's `## Handoff citations` block:\n\n\
         - For a section, write `<path>#<Heading>`.\n\
         - For a numbered item, write `<path>#<Rn>` (or `#<Cn>` / `#<Qn>`).\n\
         - For anything you did not use, write `not-used: <id> — <one-line reason>`.\n\
         Cite every heading and every numbered item. Do not silently drop or \
         merge items.\n",
    );
    for injected in artifacts {
        let record = &injected.artifact.record;
        let path = store_relative_path(&injected.topic);
        block.push_str(&format!(
            "\n### {path}\n\n- Path: `{path}`\n- Author role: `{}`\n- Phase: `{}`\n- Type: `{}`\n\n{}\n",
            record.author_role,
            record.phase,
            artifact_type_label(record.artifact_type),
            injected.artifact.body.trim_end(),
        ));
    }
    block
}

fn artifact_type_label(artifact_type: ArtifactType) -> &'static str {
    match artifact_type {
        ArtifactType::Plan => "plan",
        ArtifactType::Analysis => "analysis",
        ArtifactType::Decision => "decision",
        ArtifactType::Report => "report",
        ArtifactType::Content => "content",
    }
}

/// Stable citable elements of an artifact: every top-level heading and every
/// `R`/`C`/`Q`-numbered item ID. This is the coverage checklist from §5 of
/// the contract, nothing more.
pub fn citable_elements(artifact: &Artifact) -> Vec<String> {
    let mut elements = Vec::new();
    for line in artifact.body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            elements.push(heading.trim().to_string());
        }
    }
    static ITEM_ID: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let item_id = ITEM_ID
        .get_or_init(|| regex::Regex::new(r"\b([RCQ][0-9]+)\b").expect("item-id pattern is valid"));
    for capture in item_id.captures_iter(&artifact.body) {
        let id = capture[1].to_string();
        if !elements.contains(&id) {
            elements.push(id);
        }
    }
    elements
}

/// Extract the last `## Handoff citations` block from a consumer's final
/// message. Last-one-wins matches the executor's other markers: reasoning may
/// quote the format before the real block.
fn citation_block(text: &str) -> Option<&str> {
    const MARKER: &str = "## Handoff citations";
    let start = text.rfind(MARKER)? + MARKER.len();
    let rest = &text[start..];
    let end = rest
        .find("\n## ")
        .map(|index| index + 1)
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Coverage-only check of a consumer's citation block against an artifact.
/// A token is `cited` when it appears in a citation line, `not_used` when its
/// line carries the `not-used` marker, and `missing` otherwise.
pub fn citation_coverage(artifact: &Artifact, response: &str) -> CitationCoverage {
    let Some(block) = citation_block(response) else {
        return CitationCoverage {
            missing: citable_elements(artifact),
            ..CitationCoverage::default()
        };
    };
    let lines = block.lines().collect::<Vec<_>>();
    let mut coverage = CitationCoverage::default();
    for element in citable_elements(artifact) {
        let matching: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|line| line_mentions(line, &element))
            .collect();
        if matching.is_empty() {
            coverage.missing.push(element);
        } else if matching.iter().all(|line| line.contains("not-used")) {
            coverage.not_used.push(element);
        } else {
            coverage.cited.push(element);
        }
    }
    coverage
}

/// Whether a citation line references `element`. Cited lines use the
/// `<path>#<Heading>` / `<path>#<Rn>` form; `not-used:` lines may name the
/// bare ID (contract §5's `not-used: <id> — <reason>`).
fn line_mentions(line: &str, element: &str) -> bool {
    if line.contains(&format!("#{element}")) {
        return true;
    }
    if !line.contains("not-used") {
        return false;
    }
    line.match_indices(element).any(|(index, _)| {
        let before_ok = line[..index]
            .chars()
            .last()
            .is_none_or(|character| !character.is_alphanumeric());
        let after_ok = line[index + element.len()..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_alphanumeric());
        before_ok && after_ok
    })
}

/// Coverage roll-up for one handoff, recorded in the ledger.
pub fn citation_result(artifact: &Artifact, response: &str) -> CitationResult {
    citation_coverage(artifact, response).result()
}

/// Append handoff attribution records to the project ledger.
pub fn record_handoffs(repo_path: &Path, entries: &[HandoffLedgerEntry]) -> Result<(), AppError> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut ledger = load_ledger(repo_path)?;
    ledger.handoffs.extend(entries.iter().cloned());
    save_ledger(repo_path, &ledger)
}

/// Handoffs recorded for one run plan, in ledger order.
pub fn handoffs_for_run(
    repo_path: &Path,
    run_id: &str,
) -> Result<Vec<HandoffLedgerEntry>, AppError> {
    let ledger = load_ledger(repo_path)?;
    Ok(ledger
        .handoffs
        .into_iter()
        .filter(|entry| entry.run_id.as_deref() == Some(run_id))
        .collect())
}

/// Validate and persist a new artifact. Existing topics are deliberately not
/// replaceable: retries must fail while preserving the original handoff.
pub fn save_artifact(repo_path: &Path, artifact: &Artifact) -> Result<(), AppError> {
    // Normalize before validation so a directly-constructed artifact gets the
    // same schema headings a copied agent artifact does.
    let mut artifact = artifact.clone();
    normalize_schema_headings(&mut artifact);
    let artifact = &artifact;
    validate_artifact(artifact, None)?;
    let destination = artifact_path(repo_path, &artifact.record.artifact)?;
    if destination.exists() {
        return Err(AppError::Conflict(format!(
            "handoff artifact \"{}\" already exists; original preserved",
            artifact.record.artifact
        )));
    }
    let contents = render_artifact(artifact)?;
    crate::persist::atomic_write(&destination, &contents)?;
    let mut ledger = load_ledger(repo_path)?;
    ledger.nodes.push(HandoffLedgerNode {
        topic: artifact.record.artifact.clone(),
        path: destination
            .strip_prefix(repo_path)
            .unwrap_or(&destination)
            .to_string_lossy()
            .into_owned(),
        author_role: artifact.record.author_role.clone(),
        phase: artifact.record.phase.clone(),
        artifact_type: artifact.record.artifact_type,
    });
    ledger.links.extend(
        artifact
            .record
            .cites
            .iter()
            .map(|source| HandoffLedgerLink {
                from_topic: source.clone(),
                to_topic: artifact.record.artifact.clone(),
                relation: "cites".into(),
            }),
    );
    save_ledger(repo_path, &ledger)?;
    Ok(())
}

/// Validate and copy the authoritative file written by the agent into the
/// persistent store. The source is never removed or modified.
pub fn copy_agent_artifact(
    repo_path: &Path,
    worktree: &Path,
    phase: &str,
    topic: &str,
) -> Result<Artifact, AppError> {
    let topic = topic_for_phase(topic);
    if topic.is_empty() {
        return Err(AppError::RunPlan(format!(
            "phase \"{phase}\" has no configured handoff topic"
        )));
    }
    let source = agent_artifact_path(worktree, &topic)?;
    if !source.is_file() {
        return Err(AppError::RunPlan(format!(
            "phase \"{phase}\" completed without its handoff artifact at {}",
            source.display()
        )));
    }
    let contents = std::fs::read_to_string(&source)?;
    let mut artifact = parse_artifact(&contents, &topic, source)?;
    if artifact.record.phase != phase {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} declares phase \"{}\", expected \"{phase}\"",
            artifact.record.artifact, artifact.record.phase
        )));
    }
    let destination = artifact_path(repo_path, &topic)?;
    artifact.record.path = destination;
    save_artifact(repo_path, &artifact)?;
    Ok(artifact)
}

/// Copy the persistent artifact store into a phase worktree's
/// `.loopdeck/handoffs/` before that phase's turn starts.
///
/// Every phase worktree is cut from the default branch, so a dependent phase
/// cannot otherwise see an upstream artifact the executor copied into the
/// main checkout minutes earlier. Seeding makes the dependency explicit:
/// downstream sessions read only committed handoff contracts, never an
/// upstream branch's unmerged code (the approved branch-scoping answer).
///
/// Existing files in the worktree are never overwritten. Returns how many
/// artifacts were copied; a missing store is a no-op.
pub fn seed_worktree_store(repo_path: &Path, worktree: &Path) -> Result<usize, AppError> {
    let store = store_dir(repo_path);
    if !store.is_dir() {
        return Ok(0);
    }
    let destination = worktree.join(".loopdeck").join(HANDOFFS_DIR);
    let mut copied = 0;
    for entry in std::fs::read_dir(&store)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        std::fs::create_dir_all(&destination)?;
        let target = destination.join(name);
        if target.exists() {
            continue;
        }
        std::fs::copy(&path, &target)?;
        copied += 1;
    }
    Ok(copied)
}

fn parse_artifact(contents: &str, topic: &str, path: PathBuf) -> Result<Artifact, AppError> {
    let Some(rest) = contents.strip_prefix("---\n") else {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing YAML frontmatter",
            path.display()
        )));
    };
    let Some(end) = rest.find("\n---\n") else {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} has an unterminated YAML frontmatter block",
            path.display()
        )));
    };
    let frontmatter = &rest[..end];
    let body = &rest[end + "\n---\n".len()..];
    if frontmatter.len() > MAX_FRONTMATTER_BYTES {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} frontmatter exceeds 1 KiB",
            path.display()
        )));
    }
    let record: ArtifactRecord = serde_yaml::from_str(frontmatter)?;
    let mut artifact = Artifact {
        record: ArtifactRecord { path, ..record },
        body: body.to_string(),
    };
    normalize_schema_headings(&mut artifact);
    validate_artifact(&artifact, Some(topic))?;
    Ok(artifact)
}

/// Rewrite the producer's body into the contract's schema headings. The spike
/// observed producer-side heading drift (free-form `##` sections instead of
/// `## Summary` / `## Requirements`), so the executor normalizes here rather
/// than trusting the agent: known aliases are canonicalized, unknown
/// top-level headings are demoted (they cannot masquerade as schema
/// sections), and a missing `## Summary` is generated from the record's
/// one-line summary. Content is preserved, never dropped.
fn normalize_schema_headings(artifact: &mut Artifact) {
    struct Section {
        heading: String,
        body: String,
    }

    // Preserve the producer's trailing-newline convention: an artifact that
    // did not end with a newline must round-trip byte-identically.
    let ended_with_newline = artifact.body.ends_with('\n');
    let mut preamble = String::new();
    let mut sections: Vec<Section> = Vec::new();
    for line in artifact.body.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            let heading = match canonical_section(title) {
                Some(canonical) => format!("## {canonical}"),
                None => format!("### {}", title.trim()),
            };
            sections.push(Section {
                heading,
                body: String::new(),
            });
        } else if line.starts_with("# ") {
            // A document title above the schema sections: nest it so it can
            // never be mistaken for one of the contract's top-level headings.
            let nested = format!("##{line}");
            match sections.last_mut() {
                Some(section) => {
                    section.body.push_str(&nested);
                    section.body.push('\n');
                }
                None => {
                    preamble.push_str(&nested);
                    preamble.push('\n');
                }
            }
        } else {
            match sections.last_mut() {
                Some(section) => {
                    section.body.push_str(line);
                    section.body.push('\n');
                }
                None => {
                    preamble.push_str(line);
                    preamble.push('\n');
                }
            }
        }
    }

    match sections
        .iter()
        .position(|section| section.heading == "## Summary")
    {
        Some(index) => {
            if !preamble.is_empty() {
                sections[index].body = format!("{preamble}{}", sections[index].body);
            }
        }
        None => {
            let mut body = artifact.record.summary.clone();
            if !preamble.is_empty() {
                body.push_str("\n\n");
                body.push_str(&preamble);
            }
            body.push('\n');
            sections.insert(
                0,
                Section {
                    heading: "## Summary".into(),
                    body,
                },
            );
        }
    }

    let mut body = String::new();
    for section in sections {
        body.push_str(&section.heading);
        body.push('\n');
        body.push_str(&section.body);
    }
    if ended_with_newline {
        if !body.ends_with('\n') {
            body.push('\n');
        }
    } else {
        while body.ends_with('\n') {
            body.pop();
        }
    }
    artifact.body = body;
}

/// Map a free-form heading to the contract's canonical section name. Matching
/// is case-insensitive and tolerant of whitespace and a trailing colon.
fn canonical_section(title: &str) -> Option<&'static str> {
    let normalized = title.trim().trim_end_matches(':').to_ascii_lowercase();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    match normalized.as_str() {
        "summary" | "overview" | "description" | "goal" | "goals" => Some("Summary"),
        "requirement" | "requirements" | "scope" => Some("Requirements"),
        "constraint" | "constraints" | "limitation" | "limitations" => Some("Constraints"),
        "non-goal" | "non-goals" | "non goal" | "non goals" => Some("Non-Goals"),
        "open question" | "open questions" | "question" | "questions" => Some("Open Questions"),
        _ => None,
    }
}

fn validate_artifact(artifact: &Artifact, expected_topic: Option<&str>) -> Result<(), AppError> {
    let record = &artifact.record;
    let topic = topic_for_phase(&record.artifact);
    if topic != record.artifact || record.artifact.is_empty() {
        return Err(AppError::RunPlan(format!(
            "handoff artifact topic \"{}\" must be a kebab-case slug",
            record.artifact
        )));
    }
    if expected_topic.is_some_and(|expected| expected != record.artifact) {
        return Err(AppError::RunPlan(format!(
            "handoff artifact topic \"{}\" does not match expected \"{}\"",
            record.artifact,
            expected_topic.unwrap_or_default()
        )));
    }
    if record.author_role.trim().is_empty() || record.phase.trim().is_empty() {
        return Err(AppError::RunPlan(
            "handoff artifact author_role and phase are required".into(),
        ));
    }
    if record.summary.chars().count() > 200 || record.summary.contains('\n') {
        return Err(AppError::RunPlan(
            "handoff artifact summary must be one line and at most 200 characters".into(),
        ));
    }
    if artifact.body.len() > MAX_BODY_BYTES {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} body exceeds 8 KiB",
            record.artifact
        )));
    }
    let headings = artifact
        .body
        .lines()
        .filter(|line| line.starts_with("## "))
        .map(|line| line.trim_start_matches("## ").trim())
        .collect::<Vec<_>>();
    if !headings.contains(&"Summary") {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing the required ## Summary section",
            record.artifact
        )));
    }
    if matches!(
        record.artifact_type,
        ArtifactType::Plan | ArtifactType::Analysis
    ) && !headings.contains(&"Requirements")
    {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing the required ## Requirements section",
            record.artifact
        )));
    }
    if record.artifact_type == ArtifactType::Plan && !headings.contains(&"Non-Goals") {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing the required ## Non-Goals section",
            record.artifact
        )));
    }
    let sections = headings.len();
    if sections > 8 {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} has more than 8 top-level sections",
            record.artifact
        )));
    }
    let numbered_items = artifact
        .body
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.as_bytes().first().is_some_and(u8::is_ascii_digit) && trimmed.contains(". ")
        })
        .count();
    if numbered_items > 12 {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} has more than 12 numbered items",
            record.artifact
        )));
    }
    Ok(())
}

fn render_artifact(artifact: &Artifact) -> Result<String, AppError> {
    let frontmatter = serde_yaml::to_string(&artifact.record)?;
    let frontmatter = frontmatter
        .lines()
        .filter(|line| !line.starts_with("path:"))
        .collect::<Vec<_>>()
        .join("\n");
    if frontmatter.len() > MAX_FRONTMATTER_BYTES {
        return Err(AppError::RunPlan(
            "handoff frontmatter exceeds 1 KiB".into(),
        ));
    }
    Ok(format!("---\n{frontmatter}\n---\n{}", artifact.body))
}

pub fn load(repo_path: &Path) -> Result<Option<HandoffRecord>, AppError> {
    let path = handoff_path(repo_path);
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)?;
    Ok(serde_yaml::from_str(&contents)?)
}

pub fn save(repo_path: &Path, record: &HandoffRecord) -> Result<(), AppError> {
    let path = handoff_path(repo_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_yaml::to_string(record)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn record() -> HandoffRecord {
        HandoffRecord {
            delivered_branch: "run/x-abc".into(),
            pr_url: "https://github.com/o/r/pull/9".into(),
            worktree: PathBuf::from("/repo/.loopdeck/runs/run/x-abc"),
            next_base: "main".into(),
            delivered_at: Utc.with_ymd_and_hms(2026, 8, 31, 9, 0, 0).unwrap(),
        }
    }

    #[test]
    fn roundtrips_through_yaml() {
        let dir = std::env::temp_dir().join(format!("loopdeck-handoff-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        assert!(load(&dir).unwrap().is_none());

        save(&dir, &record()).unwrap();
        assert_eq!(load(&dir).unwrap(), Some(record()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_creates_the_loopdeck_dir() {
        let dir = std::env::temp_dir()
            .join(format!("loopdeck-handoff-nested-{}", uuid::Uuid::new_v4()))
            .join("repo");
        std::fs::create_dir_all(&dir).unwrap();

        save(&dir, &record()).unwrap();
        assert!(handoff_path(&dir).exists());

        std::fs::remove_dir_all(dir.parent().unwrap()).ok();
    }

    fn artifact(topic: &str, phase: &str, artifact_type: ArtifactType) -> Artifact {
        Artifact {
            record: ArtifactRecord {
                artifact: topic.into(),
                author_role: "engineering-manager".into(),
                phase: phase.into(),
                artifact_type,
                created: chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                summary: "A valid handoff artifact".into(),
                cites: vec![],
                path: PathBuf::new(),
            },
            body: "## Summary\nA summary.\n\n## Requirements\n1. R1 is stable.\n\n## Non-Goals\nNothing else.".into(),
        }
    }

    #[test]
    fn artifact_roundtrips_with_record_path_and_body() {
        let dir = std::env::temp_dir().join(format!("loopdeck-artifact-{}", uuid::Uuid::new_v4()));
        let original = artifact(
            "store-model",
            "agent-handoff/store-model",
            ArtifactType::Plan,
        );
        save_artifact(&dir, &original).unwrap();

        let loaded = load_artifact(&dir, "store-model").unwrap().unwrap();
        assert_eq!(loaded.record.artifact, "store-model");
        assert_eq!(loaded.record.author_role, "engineering-manager");
        assert_eq!(loaded.record.artifact_type, ArtifactType::Plan);
        assert_eq!(loaded.record.phase, original.record.phase);
        assert_eq!(loaded.body, original.body);
        assert_eq!(
            loaded.record.path,
            artifact_path(&dir, "store-model").unwrap()
        );
        let ledger = load_ledger(&dir).unwrap();
        assert_eq!(ledger.nodes.len(), 1);
        assert_eq!(ledger.nodes[0].topic, "store-model");
        assert_eq!(ledger.nodes[0].author_role, "engineering-manager");
        assert!(ledger.links.is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn ledger_roundtrips_as_plain_json_nodes_and_links() {
        let dir = std::env::temp_dir().join(format!("loopdeck-ledger-{}", uuid::Uuid::new_v4()));
        let ledger = HandoffLedger {
            nodes: vec![HandoffLedgerNode {
                topic: "upstream-plan".into(),
                path: ".loopdeck/handoffs/upstream-plan.md".into(),
                author_role: "planner".into(),
                phase: "epic/plan".into(),
                artifact_type: ArtifactType::Plan,
            }],
            links: vec![HandoffLedgerLink {
                from_topic: "upstream-plan".into(),
                to_topic: "downstream-analysis".into(),
                relation: "cites".into(),
            }],
            handoffs: vec![HandoffLedgerEntry {
                run_id: Some("run-1".into()),
                from_role: "planner".into(),
                to_role: "qa".into(),
                artifact_path: ".loopdeck/handoffs/upstream-plan.md".into(),
                citation: CitationResult::Pass,
            }],
        };
        save_ledger(&dir, &ledger).unwrap();
        assert_eq!(load_ledger(&dir).unwrap(), ledger);
        let raw = std::fs::read_to_string(ledger_path(&dir)).unwrap();
        assert!(raw.contains("\"nodes\""));
        assert!(raw.contains("\"links\""));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn duplicate_topic_fails_without_replacing_original() {
        let dir = std::env::temp_dir().join(format!("loopdeck-artifact-{}", uuid::Uuid::new_v4()));
        let original = artifact(
            "retry-topic",
            "agent-handoff/store-model",
            ArtifactType::Report,
        );
        save_artifact(&dir, &original).unwrap();

        let mut retry = artifact(
            "retry-topic",
            "agent-handoff/store-model",
            ArtifactType::Report,
        );
        retry.body = "## Summary\nA different artifact.".into();
        let error = save_artifact(&dir, &retry).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            load_artifact(&dir, "retry-topic").unwrap().unwrap().body,
            original.body
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn agent_artifact_is_validated_and_copied_without_removing_source() {
        let root = std::env::temp_dir().join(format!("loopdeck-artifact-{}", uuid::Uuid::new_v4()));
        let worktree = root.join("worktree");
        let source = worktree.join(".loopdeck/handoff-artifacts/agent-handoff-store-model.md");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        let written = artifact(
            "agent-handoff-store-model",
            "agent-handoff/store-model",
            ArtifactType::Plan,
        );
        std::fs::write(&source, render_artifact(&written).unwrap()).unwrap();

        let copied = copy_agent_artifact(
            &root,
            &worktree,
            "agent-handoff/store-model",
            "agent-handoff-store-model",
        )
        .unwrap();
        assert_eq!(
            copied.record.path,
            artifact_path(&root, "agent-handoff-store-model").unwrap()
        );
        assert!(source.exists());
        assert!(artifact_path(&root, "agent-handoff-store-model")
            .unwrap()
            .exists());

        std::fs::remove_dir_all(root).ok();
    }

    fn copy_from_worktree(
        root: &Path,
        worktree: &Path,
        contents: &str,
    ) -> Result<Artifact, AppError> {
        let source = worktree.join(".loopdeck/handoff-artifacts/drifted.md");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, contents).unwrap();
        copy_agent_artifact(root, worktree, "agent-handoff/artifact-write", "drifted")
    }

    /// The executor, not the agent, owns the schema headings (spike finding:
    /// producers drifted into free-form `##` sections).
    #[test]
    fn copy_normalizes_drifted_headings_into_the_schema() {
        let root = std::env::temp_dir().join(format!("loopdeck-drift-{}", uuid::Uuid::new_v4()));
        let worktree = root.join("worktree");
        let copied = copy_from_worktree(
            &root,
            &worktree,
            "---\nartifact: drifted\nauthor_role: planner\nphase: agent-handoff/artifact-write\ntype: report\ncreated: 2026-10-28\nsummary: Notes from the phase.\n---\n## Overview\nHigh level.\n\n## Findings\n- one\n- two\n",
        )
        .expect("drifted artifact is normalized, not rejected");

        assert!(copied.body.starts_with("## Summary\nHigh level.\n"));
        assert!(copied.body.contains("High level."));
        assert!(copied.body.contains("### Findings"));
        assert!(!copied.body.contains("\n## Findings"));
        assert_eq!(
            load_artifact(&root, "drifted").unwrap().unwrap().body,
            copied.body
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn copy_generates_summary_when_the_producer_omits_one() {
        let root = std::env::temp_dir().join(format!("loopdeck-summary-{}", uuid::Uuid::new_v4()));
        let worktree = root.join("worktree");
        let copied = copy_from_worktree(
            &root,
            &worktree,
            "---\nartifact: drifted\nauthor_role: planner\nphase: agent-handoff/artifact-write\ntype: report\ncreated: 2026-10-28\nsummary: One-line summary from frontmatter.\n---\nPlain body without headings.\n",
        )
        .expect("missing Summary is generated mechanically");

        assert!(copied
            .body
            .starts_with("## Summary\nOne-line summary from frontmatter.\n\nPlain body"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn copy_rejects_a_missing_configured_topic() {
        let root = std::env::temp_dir().join(format!("loopdeck-topic-{}", uuid::Uuid::new_v4()));
        let error = copy_agent_artifact(&root, &root, "agent-handoff/artifact-write", "")
            .expect_err("empty topic must not derive one from the phase id");
        assert!(error.to_string().contains("no configured handoff topic"));
    }

    #[test]
    fn cites_become_ledger_links() {
        let dir = std::env::temp_dir().join(format!("loopdeck-cites-{}", uuid::Uuid::new_v4()));
        let mut upstream = artifact(
            "upstream",
            "agent-handoff/store-model",
            ArtifactType::Report,
        );
        save_artifact(&dir, &upstream).unwrap();
        upstream.record.artifact = "downstream".into();
        upstream.record.cites = vec!["upstream".into()];
        upstream.record.author_role = "qa".into();
        save_artifact(&dir, &upstream).unwrap();

        let ledger = load_ledger(&dir).unwrap();
        assert_eq!(ledger.nodes.len(), 2);
        assert_eq!(ledger.links.len(), 1);
        assert_eq!(ledger.links[0].from_topic, "upstream");
        assert_eq!(ledger.links[0].to_topic, "downstream");
        assert_eq!(ledger.links[0].relation, "cites");
        std::fs::remove_dir_all(dir).ok();
    }

    // ── Downstream injection (`prd-agent-handoff` Phase 2) ──────────────

    #[test]
    fn render_injection_names_the_store_path_role_and_citation_rule() {
        let plan = artifact("upstream-plan", "epic/plan", ArtifactType::Plan);
        let block = render_injection(
            "epic/implement",
            &[InjectedArtifact {
                topic: "upstream-plan".into(),
                artifact: plan,
            }],
        );

        assert!(block.contains("## Upstream handoff artifacts for phase `epic/implement`"));
        assert!(block.contains(".loopdeck/handoffs/upstream-plan.md"));
        assert!(block.contains("Author role: `engineering-manager`"));
        assert!(block.contains("## Handoff citations"));
        assert!(block.contains("<path>#<Heading>"));
        assert!(block.contains("not-used: <id> — <one-line reason>"));
        assert!(block.contains("R1 is stable"));
    }

    #[test]
    fn injection_load_is_bounded_at_the_contract_hard_cap() {
        let dir = std::env::temp_dir().join(format!("loopdeck-inject-{}", uuid::Uuid::new_v4()));
        let store = store_dir(&dir);
        std::fs::create_dir_all(&store).unwrap();

        // Exactly 8 KiB is allowed; one byte more is an error, never truncated.
        let at_cap = format!(
            "---\nartifact: bounded\nauthor_role: planner\nphase: x\ntype: report\ncreated: 2026-10-28\nsummary: s\n---\n## Summary\n{}",
            "a".repeat(MAX_INJECTION_BYTES - "## Summary\n".len())
        );
        assert_eq!(
            at_cap.split("---\n").nth(2).unwrap().len(),
            MAX_INJECTION_BYTES
        );
        std::fs::write(store.join("bounded.md"), &at_cap).unwrap();
        let loaded = load_for_injection(&dir, "bounded").expect("8 KiB body is allowed");
        assert_eq!(loaded.body.len(), MAX_INJECTION_BYTES);

        std::fs::write(store.join("bounded.md"), format!("{at_cap}b")).unwrap();
        let error = load_for_injection(&dir, "bounded").expect_err("over-cap artifact is an error");
        assert!(error.to_string().contains("8 KiB"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn injection_load_reports_a_missing_artifact() {
        let dir = std::env::temp_dir().join(format!("loopdeck-missing-{}", uuid::Uuid::new_v4()));
        let error = load_for_injection(&dir, "never-written")
            .expect_err("missing artifact must never be improvised");
        assert!(error.to_string().contains("is missing from the store"));
        std::fs::remove_dir_all(dir).ok();
    }

    // ── Citation coverage (`prd-agent-handoff` Phase 3) ─────────────────

    fn response_with_citations(block: &str) -> String {
        format!("Implementation done.\n\n## Handoff citations\n{block}\n")
    }

    #[test]
    fn citation_coverage_splits_cited_not_used_and_missing() {
        let plan = artifact("upstream", "epic/plan", ArtifactType::Plan);
        let response = response_with_citations(
            "- .loopdeck/handoffs/upstream.md#Summary\n\
             - .loopdeck/handoffs/upstream.md#Requirements\n\
             - not-used: R1 — out of scope\n\
             - .loopdeck/handoffs/upstream.md#Non-Goals",
        );

        let coverage = citation_coverage(&plan, &response);
        assert_eq!(coverage.cited, vec!["Summary", "Requirements", "Non-Goals"]);
        assert_eq!(coverage.not_used, vec!["R1"]);
        assert!(coverage.missing.is_empty());
        assert_eq!(coverage.result(), CitationResult::Pass);
    }

    #[test]
    fn citation_coverage_is_partial_when_some_elements_are_absent() {
        let plan = artifact("upstream", "epic/plan", ArtifactType::Plan);
        let response = response_with_citations("- .loopdeck/handoffs/upstream.md#Summary");

        let coverage = citation_coverage(&plan, &response);
        assert_eq!(coverage.cited, vec!["Summary"]);
        assert!(coverage.missing.contains(&"Requirements".to_string()));
        assert!(coverage.missing.contains(&"R1".to_string()));
        assert_eq!(coverage.result(), CitationResult::Partial);
    }

    #[test]
    fn citation_coverage_is_missing_without_a_block() {
        let plan = artifact("upstream", "epic/plan", ArtifactType::Plan);
        let coverage = citation_coverage(&plan, "No citation block here.");
        assert!(coverage.cited.is_empty());
        assert_eq!(coverage.missing.len(), 4);
        assert_eq!(coverage.result(), CitationResult::Missing);
    }

    #[test]
    fn citation_block_stops_at_the_next_top_level_heading() {
        let plan = artifact("upstream", "epic/plan", ArtifactType::Plan);
        let response = "\n## Handoff citations\n- .loopdeck/handoffs/upstream.md#Summary\n\
                        \n## Other section\n- .loopdeck/handoffs/upstream.md#R1\n";
        let coverage = citation_coverage(&plan, response);
        assert_eq!(coverage.cited, vec!["Summary"]);
        assert!(coverage.missing.contains(&"R1".to_string()));
    }

    #[test]
    fn handoffs_for_run_filters_the_project_ledger_by_run_id() {
        let dir = std::env::temp_dir().join(format!("loopdeck-attrib-{}", uuid::Uuid::new_v4()));
        let entry = |run_id: &str| HandoffLedgerEntry {
            run_id: Some(run_id.into()),
            from_role: "planner".into(),
            to_role: "qa".into(),
            artifact_path: ".loopdeck/handoffs/upstream.md".into(),
            citation: CitationResult::Pass,
        };
        record_handoffs(&dir, &[entry("run-1")]).unwrap();
        record_handoffs(&dir, &[entry("run-2"), entry("run-1")]).unwrap();

        let run_one = handoffs_for_run(&dir, "run-1").unwrap();
        assert_eq!(run_one.len(), 2);
        let run_two = handoffs_for_run(&dir, "run-2").unwrap();
        assert_eq!(run_two.len(), 1);
        assert!(handoffs_for_run(&dir, "run-3").unwrap().is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn ledger_without_handoff_entries_still_loads() {
        let dir = std::env::temp_dir().join(format!("loopdeck-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(store_dir(&dir)).unwrap();
        std::fs::write(ledger_path(&dir), "{\"nodes\":[],\"links\":[]}").unwrap();

        let ledger = load_ledger(&dir).unwrap();
        assert!(ledger.handoffs.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}
