import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { RunQueueEvent } from "../types";
import { usePendingInteractions } from "../store/pendingInteractions";
import { useStreamingState } from "../store/streamingState";

const RUN_QUEUE_EVENT = "run-queue-event";

/**
 * Bridge detached run-queue activity into the same navigation-stable stores
 * used by the interactive Agent surface. Mounted once at the app root so it
 * keeps receiving every project's events across all navigation.
 */
export function useRunQueueEvents() {
  useEffect(() => {
    let disposed = false;
    const unlisten = listen<RunQueueEvent>(RUN_QUEUE_EVENT, ({ payload }) => {
      if (disposed) return;

      const projectPath = payload.project_path;
      const event = payload.event;
      const streaming = useStreamingState.getState();
      if (!streaming.get(projectPath).busy && event.type !== "result") {
        streaming.beginTurn(projectPath);
      }
      const pending = usePendingInteractions.getState();

      switch (event.type) {
        case "text_delta":
          streaming.appendTextDelta(projectPath, event.text);
          break;
        case "thinking_delta":
          streaming.appendThinkingDelta(projectPath, event.thinking);
          break;
        case "tool_use":
          if (
            event.name !== "AskUserQuestion" &&
            event.name !== "TaskCreate" &&
            event.name !== "TaskUpdate"
          ) {
            streaming.appendBlock(projectPath, {
              type: "tool_use",
              name: event.name,
              input: event.input,
            });
          }
          break;
        case "task_update":
          streaming.applyTask(projectPath, event.task);
          break;
        case "permission_request":
          if (event.decision === "pending") {
            pending.setPermission(projectPath, {
              requestId: event.request_id,
              toolName: event.tool_name,
              input: event.input,
            });
          } else {
            pending.clearPermission(projectPath);
          }
          break;
        case "ask_user_question":
          pending.setQuestion(projectPath, {
            requestId: event.request_id,
            questions: event.questions,
          });
          break;
        case "plan_approval":
          if (event.decision === "pending") {
            pending.setPlan(projectPath, {
              requestId: event.request_id,
              plan: event.plan,
            });
          } else {
            pending.clearPlan(projectPath);
          }
          break;
        case "retrying":
          streaming.patch(projectPath, {
            retrying: {
              attempt: event.attempt,
              maxAttempts: event.max_attempts,
              backoffMs: event.backoff_ms,
              error: event.error,
            },
          });
          break;
        case "result":
          streaming.patch(projectPath, {
            busy: false,
            streamingResult: event,
            retrying: null,
            error: event.is_error ? event.result || "The unattended turn failed." : null,
          });
          pending.clearQuestion(projectPath);
          pending.clearPermission(projectPath);
          pending.clearPlan(projectPath);
          break;
      }
    });

    return () => {
      disposed = true;
      void unlisten.then((stop) => stop());
    };
  }, []);
}
