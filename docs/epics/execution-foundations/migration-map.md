# Domain Contract — Legacy Identity and Migration Map

**Status:** Phase 1 evidence, 2026-10-14  
**Pilot fixture:** the current `loopdeck` project checkout, copied before any
import

## Mapping rules

| New identity | Legacy source | Mapping |
|---|---|---|
| `ProjectId` | `.loopdeck/project.yaml` + canonical project path | Deterministic UUIDv5 of the normalized project locator for the first import; persist the assigned ID so a later move does not create a second project. A path remains a mutable locator. |
| `RunId` | `run-plan.yaml.id` | Use the existing `run-<UUID>` unchanged. |
| `RunId` | `.loopdeck/agent-runs/<UUID>/manifest.json` | Use `manifest.run.id` unchanged. |
| `RunId` | ordinary `sessions/active.jsonl` or `archive-*.jsonl` with no run ID | Generate `legacy-interactive-<sha256(project path + archive filename)>`; persist the source filename and hash so the mapping is repeatable. |
| `PhaseId` | `execution.yaml` loop ID / `run-plan.yaml.execution_id` / `PrdLoop.id` | Preserve the string exactly. Do not regenerate it from title or list position. |
| `SessionId` | `ConversationTurn.session_id` | Preserve the provider value; namespace provider values (`claude:<id>`, existing `codex:<id>`) when inserting into the new store. Missing IDs remain null. |
| `DecisionId` | pending `request_id` | Preserve while live. For already-resolved historical decisions, synthesize `legacy-decision-<sha256(run/phase + transcript line)>` and mark provenance as synthetic. |

## Compatibility reads

During import and pilot, read in this order:

1. New store snapshot, when a matching `ProjectId` and source provenance exist.
2. Existing `execution.yaml`, `run-plan.yaml`, and multi-agent manifests for
   mutable status and delivery evidence.
3. Session JSONL for transcript/session evidence and reconstruction only.
4. In-memory `AppState` for live process/pending-slot observations only.

If the projections disagree, return a structured divergence diagnostic with
both revisions and source paths. Do not silently prefer the newest timestamp:
`execution.yaml` revision and the run-plan phase status are authored by
different paths and timestamps do not establish causality.

## Pilot fixture and expected result

The fixture contains:

- one `project.yaml` with no stable project ID;
- `execution.yaml` at revision 50 with completed and abandoned loop history;
- one `run-plan.yaml` with a UUID run ID and two `execution_id` phase IDs;
- four multi-agent manifests, provider transcripts, and retained worktrees;
- active and archived conversation JSONL, including provider session IDs;
- delivery retry and handoff records.

The import must produce one project, one run for the queued plan, four
multi-agent runs, one synthetic run per ordinary conversation file, and one
phase record per unique preserved execution ID. Repeat import with unchanged
source hashes must create zero additional records and zero changed IDs.

## Data that cannot be reconstructed

- The exact lifetime of an ordinary in-memory `HarnessSession` process after
  a crash; the transcript can prove the last provider session ID, not whether
  its child was alive.
- A pending question/permission/plan payload that existed only in an in-memory
  slot and never reached a durable transcript line.
- A unique logical run ID for historical interactive conversations; the
  synthetic mapping above is stable but is explicitly marked legacy.
- Cross-file causal ordering between `execution.yaml`, `run-plan.yaml`, and
  manifests when their writes were not committed together.

These cases must import as `unknown`/`interrupted` evidence rather than being
presented as live or completed state.

## Rollback artifact

The migration creates:

```text
.loopdeck/migrations/<timestamp>/
  manifest.sha256
  project.yaml
  execution.yaml
  run-plan.yaml
  handoff.yaml                  (when present)
  delivery-retry.yaml           (when present)
  sessions/                     (active + archives)
  agent-runs/                   (manifests + transcripts)
```

The artifact is immutable after the import begins. Its manifest is the proof
that a rollback restored the exact pre-import inputs. Managed worktrees are
not copied into the artifact; their recorded path/branch is retained and Git
is responsible for recovery from the surviving branch.

## Migration stages

| Stage | New store | Existing files |
|---|---|---|
| 0. Observe | absent | sole authority |
| 1. Import | read-only imported snapshot + provenance | sole authority; no writes changed |
| 2. Pilot write | authoritative for new transitions; compatibility export written | readable and compared after every transition |
| 3. Cutover | authoritative read/write | exported for compatibility; no legacy write path removed yet |
| 4. Retirement | authoritative | retirement requires two releases with zero unexplained divergence |

