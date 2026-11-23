import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { RunEventEnvelope } from "../types/runEvent";
import {
  projectRunQueueEvent,
  type RunEventProjectionState,
} from "../lib/runEventProjection";
import { usePendingInteractions } from "../store/pendingInteractions";
import { useStreamingState } from "../store/streamingState";

/**
 * Normalized run queue event topic. The backend also keeps emitting the
 * legacy `run-queue-event` (`ClaudeEvent`) topic during the dual-emit phase;
 * this consumer migrated to the provider-neutral `RunEvent` stream first.
 */
const RUN_EVENT = "run-event";

/**
 * Bridge detached run-queue activity into the navigation-stable stores.
 *
 * All semantic interpretation lives in the pure `projectRunEvent` reducer so
 * it can be contract-tested with `node:test`; this hook only translates the
 * resulting projection into the existing Zustand store operations.
 *
 * Mounted once at the app root so it keeps receiving every project's events
 * across all navigation.
 */
export function useRunQueueEvents() {
  useEffect(() => {
    let disposed = false;
    const projections = new Map<string, {
      runId: string;
      phases: Map<string, RunEventProjectionState>;
    }>();

    const unlisten = listen<RunEventEnvelope>(RUN_EVENT, ({ payload }) => {
      if (disposed) return;

      const projectPath = payload.project_path;
      const current = projections.get(projectPath);
      const runId = payload.event.run_id;
      if (current && current.runId !== runId && payload.execution_id === "") return;
      const phases = current?.runId === runId
        ? current.phases
        : new Map<string, RunEventProjectionState>();
      const next = projectRunQueueEvent(phases, payload);
      if (!next) return;
      if (phases.size > 0) projections.set(projectPath, { runId, phases });
      else projections.delete(projectPath);

      // Keep the legacy store shape so existing render components (Chat,
      // TaskPanel, pending-card surfaces) work unchanged during migration.
      useStreamingState.getState().patch(projectPath, {
        busy: next.busy,
        streamingBlocks: next.busy || next.blocks.length > 0 ? next.blocks : null,
        streamingResult: next.result,
        error: next.error,
        tasks: next.tasks,
        retrying: next.retrying,
      });

      const pending = usePendingInteractions.getState();
      if (next.permission) {
        pending.setPermission(projectPath, next.permission);
      } else {
        pending.clearPermission(projectPath);
      }
      if (next.question) {
        pending.setQuestion(projectPath, next.question);
      } else {
        pending.clearQuestion(projectPath);
      }
      if (next.plan) {
        pending.setPlan(projectPath, next.plan);
      } else {
        pending.clearPlan(projectPath);
      }
    });

    return () => {
      disposed = true;
      void unlisten.then((stop) => stop());
    };
  }, []);
}
