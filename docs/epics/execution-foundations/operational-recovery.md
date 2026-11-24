# Central operational state and interrupted-run recovery

Implemented slice: queued unattended runs persist run plans and execution
snapshots in `operational.sqlite3` under `directories::ProjectDirs`' per-user
local data directory (`com`, `loopdeck`, `LoopDeck`). The database is outside
the repository. Authored project instructions, specifications, and decisions
remain repository files.

## Ownership and migration

- `operational_documents` stores schema-versioned run-plan and execution
  snapshots, with import source and timestamps. This is an incremental storage
  boundary, not the complete normalized operational-store schema.
- Linked worktree roots share the main checkout's operational state and
  active-run checks. The main checkout supplies the initial legacy YAML.
  Separately registered subdirectories keep their own operational state.
- A missing central record imports a validated legacy document once. Legacy
  files are retained unchanged; subsequent runtime writes go to SQLite.
- Execution revision checks run inside a write transaction. SQLite writes
  have a bounded busy timeout; short in-process operations are serialized.
- Project identity currently follows the canonical main-checkout path.
  Repository relocation and separate Git-directory layouts need a later
  stable-ID/alias migration.

## Continue an unfinished queued loop

After restart, the scheduler requeues running phases with a retained worktree
and saved session. It parks the other interrupted phases. Use **Continue
unfinished** in the run queue to retry parked phases. Completed and delivered
phases stay complete. Continuation reuses the recorded worktree and its edits
when the worktree survives.

Each phase retains its dispatch count, native provider session ID, and previous
failure/park reason. Provider start events checkpoint the session before a final
response exists. Continuation supplies the queued task contract plus recovery
instructions to inspect existing files/diffs and complete only remaining work.
Each phase runs in a separate turn. Plans with a legacy shared worktree run one
phase at a time so two phases cannot write to that worktree concurrently.

The total wall-clock budget includes elapsed time saved by earlier attempts.
Raise the limit before continuing a run that exhausted its total budget.

Compatible native sessions are resumed. Codex falls back before task dispatch
when its saved thread is explicitly missing. Claude can fall back on an explicit
missing-session response with no tool activity. Authentication, transport, and
ambiguous execution failures are surfaced; they are not blindly replayed.

## Compatibility and remaining work

- Interactive chat retains its existing native-session behavior; **Start next
  loop** still explicitly starts a fresh interactive conversation. This slice
  changes the queued-run continuation flow.
- Session JSONL, multi-agent manifests, delivery retry records, handoff files,
  and authored PRD completion bookkeeping remain on their existing paths.
- Durable event replay, pending-decision migration, normalized attempt history,
  portable exports, and database backup/restore UI remain future work.
- Before reverting to a file-only application version, stop LoopDeck and
  preserve the database together with any SQLite WAL/SHM sidecars. Legacy YAML
  is the original import snapshot, not an up-to-date export of new runs.

Recovery is covered by real temporary Git worktree/database tests and a fake
Codex transport. Live-provider interruption/resumption remains a manual smoke
test; no live model calls are made by the test suite.
