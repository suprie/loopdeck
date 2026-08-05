//! Read-only summary of a Graphify knowledge graph for a project.
//!
//! [Graphify](https://github.com/Graphify-Labs/graphify) is an external tool
//! that turns a codebase into a queryable knowledge graph. When run against a
//! project it writes `graphify-out/graph.json` (plus `graph.html` and
//! `GRAPH_REPORT.md`) into the project root.
//!
//! Selasar does **not** run Graphify and takes no dependency on Python or the
//! Graphify CLI. This module only reads the JSON metadata that Graphify, if
//! installed, has already produced — so the integration is detect-and-surface
//! only. If the file is absent or unparseable, every accessor returns a
//! `present: false` stats struct and the frontend hides the feature.
//!
//! ## Schema
//!
//! `graph.json` is a networkx node-link JSON document with these top-level
//! keys: `directed`, `multigraph`, `graph` (currently `{}`), `nodes[]`,
//! `links[]`. (Verified against `worked/httpx/graph.json`,
//! `worked/karpathy-repos/graph.json`, and `worked/mixed-corpus/graph.json`
//! in the Graphify repo, tag v8.)
//!
//! - `nodes[i]`: `{ id, label, file_type, source_file, source_location, community }`
//! - `links[i]`: `{ source, target, relation, confidence, weight, source_file,
//!   source_location, _src, _tgt }` where `confidence` ∈
//!   {`EXTRACTED`, `INFERRED`, `AMBIGUOUS`}.
//!
//! Build date and backend are not embedded in `graph.json`; they live in the
//! companion `GRAPH_REPORT.md` (first heading: `# Graph Report - <path>
//! (YYYY-MM-DD)`). We mine that line opportunistically — if absent, the fields
//! stay `None`.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Subdirectory Graphify writes into the project root.
pub const OUT_DIR: &str = "graphify-out";

/// Persistent graph data file written by Graphify.
pub const GRAPH_FILE: &str = "graph.json";

/// Human-readable companion report (mined for build date).
pub const REPORT_FILE: &str = "GRAPH_REPORT.md";

/// Interactive visualization (only the path is surfaced to the UI).
pub const HTML_FILE: &str = "graph.html";

/// Maximum number of "god nodes" (highest-degree) to surface.
const MAX_GOD_NODES: usize = 10;

/// Summary of a project's Graphify knowledge graph.
///
/// `present: false` means no readable `graphify-out/graph.json` was found —
/// the UI should hide the graph panel rather than render empty stats.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct GraphifyStats {
    /// `false` when `graphify-out/graph.json` is missing or unparseable.
    pub present: bool,
    /// Absolute count of nodes in the graph.
    pub node_count: u32,
    /// Absolute count of links (edges) in the graph.
    pub edge_count: u32,
    /// Number of distinct `community` values across nodes.
    pub community_count: u32,
    /// Labels of the highest-degree nodes (most connected first), capped at 10.
    pub god_nodes: Vec<String>,
    /// Distribution of link confidence values (key ∈ EXTRACTED/INFERRED/AMBIGUOUS).
    pub confidence: ConfidenceBreakdown,
    /// Build date parsed from `GRAPH_REPORT.md` (`YYYY-MM-DD`), if present.
    pub built_at: Option<String>,
    /// Absolute path to `graphify-out/graph.json`.
    pub graph_path: String,
    /// Absolute path to `graphify-out/GRAPH_REPORT.md`, if it exists.
    pub report_path: Option<String>,
    /// Absolute path to `graphify-out/graph.html`, if it exists.
    pub html_path: Option<String>,
}

/// Per-confidence counts for the graph's links.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ConfidenceBreakdown {
    /// Structural links extracted directly from source (AST-level certainty).
    pub extracted: u32,
    /// Links inferred by the LLM backend (plausible but not provable).
    pub inferred: u32,
    /// Links the extractor flagged as ambiguous.
    pub ambiguous: u32,
}

/// Whether a Graphify output directory with a `graph.json` exists at `root`.
///
/// Public so the scanner can badge discovered repos cheaply without parsing
/// the JSON — matching the existing `has_loopdeck` filesystem check pattern.
pub fn is_present(root: &Path) -> bool {
    graph_path(root).is_file()
}

/// Absolute path to `graphify-out/graph.json` under `root` (whether or not it
/// exists). Centralized here so callers don't rebuild the path string.
pub(crate) fn graph_path(root: &Path) -> PathBuf {
    root.join(OUT_DIR).join(GRAPH_FILE)
}

/// Read and summarize the Graphify graph for `project_root`.
///
/// Infallible: a missing or malformed graph returns
/// `GraphifyStats { present: false, .. }` (paths still populated when the
/// files exist). This mirrors `memory::parse_decisions`'s "missing file →
/// empty value" contract — the UI degrades gracefully before Graphify has
/// been run.
pub fn read_stats(project_root: &Path) -> GraphifyStats {
    let graph_path = graph_path(project_root);
    let report_path = project_root.join(OUT_DIR).join(REPORT_FILE);
    let html_path = project_root.join(OUT_DIR).join(HTML_FILE);

    let graph_path_str = graph_path.to_string_lossy().into_owned();
    let report_path_opt = report_path
        .exists()
        .then(|| report_path.to_string_lossy().into_owned());
    let html_path_opt = html_path
        .exists()
        .then(|| html_path.to_string_lossy().into_owned());

    let bytes = match std::fs::read(&graph_path) {
        Ok(b) => b,
        Err(_) => {
            return GraphifyStats {
                present: false,
                graph_path: graph_path_str,
                report_path: report_path_opt,
                html_path: html_path_opt,
                ..Default::default()
            };
        }
    };

    match summarize(&bytes) {
        Some(summary) => GraphifyStats {
            present: true,
            node_count: summary.node_count,
            edge_count: summary.edge_count,
            community_count: summary.community_count,
            god_nodes: summary.god_nodes,
            confidence: summary.confidence,
            // `GRAPH_REPORT.md` is the only place Graphify records a build
            // timestamp — mine its first heading. Best-effort: stay `None`
            // if the regex doesn't match or the file is missing.
            built_at: report_path_opt
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|content| parse_report_date(&content)),
            graph_path: graph_path_str,
            report_path: report_path_opt,
            html_path: html_path_opt,
        },
        None => GraphifyStats {
            present: false,
            graph_path: graph_path_str,
            report_path: report_path_opt,
            html_path: html_path_opt,
            ..Default::default()
        },
    }
}

/// Internal payload from a successful parse — lifted out so the happy path can
/// be unit-tested without touching the filesystem.
struct Summary {
    node_count: u32,
    edge_count: u32,
    community_count: u32,
    god_nodes: Vec<String>,
    confidence: ConfidenceBreakdown,
}

/// Parse `graph.json` bytes into a summary.
///
/// Uses `serde_json::Value` rather than a typed struct because the schema is
/// owned by an external tool and may evolve; we pluck only the fields we need
/// and ignore everything else. A `None` return means the file didn't look like
/// a Graphify graph at all (no `nodes`/`links` arrays) — the caller treats the
/// graph as not-present.
fn summarize(bytes: &[u8]) -> Option<Summary> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;

    let nodes = value.get("nodes")?.as_array()?;
    let links = value.get("links")?.as_array()?;

    let node_count = u32::try_from(nodes.len()).ok()?;
    let edge_count = u32::try_from(links.len()).ok()?;

    // Build `id → label` so we can translate the highest-degree node ids back
    // to human-readable labels. Falls back to the id if a node has no label.
    let mut id_to_label: HashMap<String, String> = HashMap::with_capacity(nodes.len());
    let mut communities: HashMap<u64, ()> = HashMap::new();
    for n in nodes {
        let id = n.get("id").and_then(|v| v.as_str()).map(String::from);
        let label = n
            .get("label")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| id.clone());
        if let (Some(id), Some(label)) = (id, label) {
            id_to_label.entry(id).or_insert(label);
        }
        if let Some(c) = n.get("community").and_then(|v| v.as_u64()) {
            communities.insert(c, ());
        }
    }
    let community_count = u32::try_from(communities.len()).ok()?;

    // Degree centrality: count every link endpoint. `source`/`target` are the
    // networkx node-link keys (Graphify emits both `source`/`target` and the
    // redundant `_src`/`_tgt`; we use the canonical ones).
    let mut degree: HashMap<String, u32> = HashMap::new();
    let mut confidence = ConfidenceBreakdown::default();
    for l in links {
        for key in ["source", "target"] {
            if let Some(id) = l.get(key).and_then(|v| v.as_str()) {
                *degree.entry(id.to_string()).or_insert(0) += 1;
            }
        }
        match l.get("confidence").and_then(|v| v.as_str()) {
            Some("EXTRACTED") => confidence.extracted += 1,
            Some("INFERRED") => confidence.inferred += 1,
            Some("AMBIGUOUS") => confidence.ambiguous += 1,
            _ => {}
        }
    }

    // God nodes: top-N by degree, ties broken by id for deterministic output.
    let mut ranked: Vec<(String, u32)> = degree.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let god_nodes = ranked
        .into_iter()
        .take(MAX_GOD_NODES)
        .map(|(id, _)| id_to_label.get(&id).cloned().unwrap_or(id))
        .collect();

    Some(Summary {
        node_count,
        edge_count,
        community_count,
        god_nodes,
        confidence,
    })
}

/// Extract `YYYY-MM-DD` from the first `# Graph Report - <path>  (YYYY-MM-DD)`
/// heading in the report. Returns the first match on its own line.
fn parse_report_date(content: &str) -> Option<String> {
    // The heading format (verified against Graphify v8 samples) is:
    //   # Graph Report - worked/httpx/raw  (2026-04-05)
    // The date may also be an ISO timestamp in some builds — we accept the
    // calendar-date prefix either way. Stay forgiving: if Graphify changes
    // the heading, this just returns `None`.
    let heading = content.lines().next()?;
    let open = heading.rfind('(')?;
    let close = heading.rfind(')')?;
    if close <= open {
        return None;
    }
    let inside = heading.get(open + 1..close)?.trim();
    // Take up to the first 10 chars (`YYYY-MM-DD`). Reject obvious non-dates.
    let date = inside.get(..10)?;
    if date.len() == 10
        && date.as_bytes()[4] == b'-'
        && date.as_bytes()[7] == b'-'
        && date[..4].chars().all(|c| c.is_ascii_digit())
        && date[5..7].chars().all(|c| c.is_ascii_digit())
        && date[8..10].chars().all(|c| c.is_ascii_digit())
    {
        Some(date.to_string())
    } else {
        None
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid graph in networkx node-link shape — a triangle of three
    /// nodes with mixed-confidence links.
    const SAMPLE_GRAPH: &str = r#"{
        "directed": false,
        "multigraph": false,
        "graph": {},
        "nodes": [
            {"id": "client", "label": "Client", "file_type": "code", "source_file": "client.py", "source_location": "L1", "community": 0},
            {"id": "models", "label": "Response", "file_type": "code", "source_file": "models.py", "source_location": "L1", "community": 1},
            {"id": "transport", "label": "HTTPTransport", "file_type": "code", "source_file": "transport.py", "source_location": "L1", "community": 1}
        ],
        "links": [
            {"source": "client", "target": "models", "relation": "imports_from", "confidence": "EXTRACTED", "weight": 1.0},
            {"source": "client", "target": "transport", "relation": "calls", "confidence": "EXTRACTED", "weight": 1.0},
            {"source": "transport", "target": "models", "relation": "imports", "confidence": "INFERRED", "weight": 0.8}
        ]
    }"#;

    #[test]
    fn summarize_parses_counts_and_communities() {
        let s = summarize(SAMPLE_GRAPH.as_bytes()).expect("sample graph should parse");
        assert_eq!(s.node_count, 3);
        assert_eq!(s.edge_count, 3);
        assert_eq!(s.community_count, 2); // communities {0, 1}
    }

    #[test]
    fn summarize_computes_degree_for_god_nodes() {
        let s = summarize(SAMPLE_GRAPH.as_bytes()).unwrap();
        // Degrees: client=2, models=2, transport=2 — all tied at 2.
        // Tie-break is alphabetical by id, so `client` ranks first.
        assert_eq!(s.god_nodes.len(), 3);
        assert_eq!(s.god_nodes[0], "Client");
        // All three labels (not ids) should appear since every node has degree 2.
        let labels: std::collections::HashSet<&str> =
            s.god_nodes.iter().map(String::as_str).collect();
        assert!(labels.contains("Response"));
        assert!(labels.contains("HTTPTransport"));
    }

    #[test]
    fn summarize_counts_confidence_breakdown() {
        let s = summarize(SAMPLE_GRAPH.as_bytes()).unwrap();
        assert_eq!(s.confidence.extracted, 2);
        assert_eq!(s.confidence.inferred, 1);
        assert_eq!(s.confidence.ambiguous, 0);
    }

    #[test]
    fn summarize_returns_none_for_non_graph_json() {
        // Valid JSON but no nodes/links arrays — not a Graphify graph.
        assert!(summarize(br#"{"hello": "world"}"#).is_none());
    }

    #[test]
    fn summarize_returns_none_for_invalid_json() {
        assert!(summarize(b"not json at all").is_none());
    }

    #[test]
    fn summarize_ignores_unknown_confidence_values() {
        let graph = r#"{
            "nodes": [{"id": "a", "label": "A", "community": 0}],
            "links": [{"source": "a", "target": "a", "confidence": "WAT"}]
        }"#;
        let s = summarize(graph.as_bytes()).unwrap();
        assert_eq!(s.confidence, ConfidenceBreakdown::default());
    }

    #[test]
    fn read_stats_missing_dir_is_not_present() {
        let tmp = tempfile_dir();
        let stats = read_stats(&tmp);
        assert!(!stats.present);
        assert_eq!(stats.node_count, 0);
        // graph_path is still populated so the UI can offer an "open" affordance.
        assert!(stats.graph_path.ends_with("graphify-out/graph.json"));
        assert!(stats.report_path.is_none());
        assert!(stats.html_path.is_none());
    }

    #[test]
    fn read_stats_malformed_graph_is_not_present_but_paths_set() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(tmp.join(OUT_DIR)).unwrap();
        std::fs::write(graph_path(&tmp), b"not json").unwrap();
        std::fs::write(tmp.join(OUT_DIR).join(HTML_FILE), b"<html></html>").unwrap();

        let stats = read_stats(&tmp);
        assert!(!stats.present);
        assert!(stats.html_path.is_some()); // the html file exists even though graph is malformed
        assert!(stats.report_path.is_none());
    }

    #[test]
    fn read_stats_valid_graph_extracts_everything() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(tmp.join(OUT_DIR)).unwrap();
        std::fs::write(graph_path(&tmp), SAMPLE_GRAPH).unwrap();
        std::fs::write(
            tmp.join(OUT_DIR).join(REPORT_FILE),
            "# Graph Report - sample  (2026-04-05)\n\n## Summary\n",
        )
        .unwrap();
        std::fs::write(tmp.join(OUT_DIR).join(HTML_FILE), b"<html></html>").unwrap();

        let stats = read_stats(&tmp);
        assert!(stats.present);
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 3);
        assert_eq!(stats.community_count, 2);
        assert_eq!(stats.built_at.as_deref(), Some("2026-04-05"));
        assert!(stats.report_path.is_some());
        assert!(stats.html_path.is_some());
    }

    #[test]
    fn read_stats_missing_report_has_no_build_date() {
        let tmp = tempfile_dir();
        std::fs::create_dir_all(tmp.join(OUT_DIR)).unwrap();
        std::fs::write(graph_path(&tmp), SAMPLE_GRAPH).unwrap();

        let stats = read_stats(&tmp);
        assert!(stats.present);
        assert!(stats.built_at.is_none());
        assert!(stats.report_path.is_none());
    }

    #[test]
    fn is_present_mirrors_file_existence() {
        let tmp = tempfile_dir();
        assert!(!is_present(&tmp));
        std::fs::create_dir_all(tmp.join(OUT_DIR)).unwrap();
        std::fs::write(graph_path(&tmp), b"{}").unwrap();
        assert!(is_present(&tmp));
    }

    #[test]
    fn parse_report_date_accepts_canonical_heading() {
        let content = "# Graph Report - worked/httpx/raw  (2026-04-05)\n## Summary";
        assert_eq!(parse_report_date(content).as_deref(), Some("2026-04-05"));
    }

    #[test]
    fn parse_report_date_rejects_non_date_parens() {
        // A report whose first line uses parens for something other than a date.
        let content = "# Graph Report - foo (final version)";
        assert!(parse_report_date(content).is_none());
    }

    #[test]
    fn parse_report_date_rejects_iso_timestamp_without_calendar_prefix() {
        // If the heading only has a non-date in parens, we don't fabricate a date.
        let content = "# Graph Report - foo (now)";
        assert!(parse_report_date(content).is_none());
    }

    /// Shared helper — a fresh unique temp dir for the test. Using
    /// `std::env::temp_dir` + a UUID-like counter keeps the test dep-free
    /// (no `tempfile` crate), matching the rest of the backend's test style.
    fn tempfile_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("loopdeck-graphify-test-{pid}-{id}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        // Best-effort cleanup on test process exit — tests don't rely on it.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("recreate temp dir");
        dir
    }

    /// End-to-end smoke test against a real Graphify sample.
    ///
    /// Skipped unless `RUN_GRAPHIFY_E2E_DIR` points at a directory containing
    /// a pre-staged `graphify-out/` (graph.json + GRAPH_REPORT.md). Download
    /// the real sample from the Graphify repo to exercise this:
    ///
    /// ```text
    /// TMP=$(mktemp -d)
    /// mkdir -p "$TMP/graphify-out"
    /// curl -sSL https://raw.githubusercontent.com/Graphify-Labs/graphify/v8/worked/httpx/graph.json \
    ///   > "$TMP/graphify-out/graph.json"
    /// curl -sSL https://raw.githubusercontent.com/Graphify-Labs/graphify/v8/worked/httpx/GRAPH_REPORT.md \
    ///   > "$TMP/graphify-out/GRAPH_REPORT.md"
    /// RUN_GRAPHIFY_E2E_DIR="$TMP" cargo test --lib read_stats_real_sample -- --ignored --nocapture
    /// ```
    ///
    /// Not a normal CI test — the schema is owned by an external tool, so this
    /// is a manual tripwire for schema drift.
    #[test]
    #[ignore]
    fn read_stats_real_sample() {
        let dir = match std::env::var("RUN_GRAPHIFY_E2E_DIR") {
            Ok(d) => PathBuf::from(d),
            Err(_) => {
                eprintln!("skipping — set RUN_GRAPHIFY_E2E_DIR to a dir with graphify-out/");
                return;
            }
        };
        let stats = read_stats(&dir);
        assert!(
            stats.present,
            "graph.json at {} should parse",
            dir.display()
        );
        assert!(
            stats.node_count > 0 && stats.edge_count > 0,
            "real sample has non-trivial graph: {stats:?}"
        );
        assert!(
            stats.community_count > 0,
            "real sample should partition into communities: {stats:?}"
        );
        assert!(
            !stats.god_nodes.is_empty(),
            "god_nodes should be computed from links: {stats:?}"
        );
        // httpx report header is `# Graph Report - worked/httpx/raw  (2026-04-05)`.
        assert_eq!(stats.built_at.as_deref(), Some("2026-04-05"));
        eprintln!("real-sample stats: {stats:?}");
    }
}
