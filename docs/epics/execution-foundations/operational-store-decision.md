# Domain Contract — Operational Store Decision

**Status:** Phase 1 evidence, 2026-10-14  
**Decision:** SQLite for the authoritative mutable operational store, with
file exports and compatibility reads retained during migration.

## Measured baseline

The measurement was taken from the canonical checkout only (nested managed
worktrees were excluded):

| Sample | Count / size | Observation |
|---|---:|---|
| `execution.yaml` | 269 lines / 11,807 bytes | 50 revisions; 22 history records in the current fixture. |
| `run-plan.yaml` | 39 lines / 1,541 bytes | 1 run, 2 phases, 2 parked statuses in the snapshot. |
| `delivery-retry.yaml` + `handoff.yaml` | 18 lines / 909 bytes | One recoverable delivery and one handoff record. |
| `agent-runs` manifests | 4 manifests | One JSONL stream is 30,299 bytes; three are one-event fixtures under 600 bytes. |
| session JSONL | 7 files / 171,052 bytes | `active.jsonl` is 44,138 bytes; the six archives total 126,914 bytes. |

The exact command used for the repeatable sample is:

```sh
find .loopdeck -maxdepth 1 -type f \( -name '*.yaml' -o -name '*.json' -o -name '*.jsonl' \) -print
find .loopdeck/sessions .loopdeck/agent-runs -type f \( -name '*.json' -o -name '*.jsonl' \) -print0 | xargs -0 wc -c -l
```

The nested worktree copies are intentionally not part of the baseline: they
are retained delivery artifacts, not additional writers for the main project.
The workload is small in the current fixture, but the important constraint is
not raw volume: concurrent UI/CLI writes, ordered append history, restart
reconciliation, and atomic multi-record transitions are currently spread over
several files.

## Comparison

| Constraint | Current files | SQLite |
|---|---|---|
| Atomic run + decision + evidence update | Requires coordinated atomic files; no cross-file transaction | One transaction with foreign keys and revision checks |
| Concurrent writers | `execution.yaml` has revision/OCC; `run-plan.yaml` and JSONL do not share a transaction | Serialized local writes with busy timeout; one authoritative writer service |
| Backup/restore | Human-readable and naturally copied with the repo; multiple files can disagree | One database file plus explicit snapshot/export; restore is bounded and testable |
| Portability | Excellent; YAML/JSONL survive repo moves | SQLite is cross-platform, but the DB must stay outside authored repo files or be explicitly portable |
| Inspection/debugging | Excellent with ordinary text tools | Requires export/diagnostic command; raw DB is less approachable |
| Migration cost | No migration | Schema/import/rollback work required |
| Ordered event history | JSONL append is simple but lacks transactional state linkage | Sequence column and unique `(run_id, sequence)` enforce ordering/idempotency |

## Decision and rejection criteria

SQLite is accepted for mutable operational state only if the implementation
also provides:

- schema versioning, integrity checks, and an explicit project identity;
- a transaction boundary for a state transition plus related decision/evidence;
- a portable export that can recreate the compatibility files;
- backup-before-import, restore verification, and a dry-run divergence report;
- a bounded busy timeout and a clear conflict error rather than silent last
  writer wins;
- no move of authored `project.yaml`, epics, PRDs, loops, or decisions into
  the database.

Reject or defer SQLite if any of these cannot be met on macOS, Linux, and
Windows packaging, or if the pilot shows unexplained projection divergence.
The fallback is to keep the file model and first centralize the transition
service around the existing `execution.yaml` revision guard.

## Backup, restore, and rollback procedure

1. Before import, copy the canonical `.loopdeck` operational files to a
   timestamped directory outside the project and record a SHA-256 manifest.
2. Import without modifying source files; write provenance (`source_path`,
   source hash, import time, and mapping version) into the new store.
3. Run integrity checks and compare the store projection with
   `execution.yaml`, `run-plan.yaml`, manifests, and transcript counts.
4. If the pilot is accepted, switch reads behind a feature flag while still
   writing compatibility exports.
5. To roll back, stop the app, disable the new read path, restore the backup
   only if a compatibility file was intentionally changed, and reopen from
   the files. The database is retained for diagnosis and is never used by the
   old path.

## Gate for the next PRD

The operational-store PRD may begin its schema/import phase after this
decision is accepted and the pilot fixture in `migration-map.md` passes a
round-trip comparison. It must not delete or rename the existing files during
the first release.

