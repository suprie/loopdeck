import type { ContentBlock, ConversationTurn } from "../../types";

/** Detect a transient gateway-overload error in a result/turn body text. */
export function isOverloadError(text: string | null | undefined): boolean {
  if (!text) return false;
  const lower = text.toLowerCase();
  return lower.includes("529") || lower.includes("overloaded");
}

export function fmtDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  const mins = Math.floor(ms / 60_000);
  const secs = Math.round((ms % 60_000) / 1000);
  return `${mins}m ${secs}s`;
}

/** Strip ANSI escape codes emitted by some streaming providers. */
export function sanitise(text: string): string {
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
}

function loopPromptSubject(text: string): string {
  const m = text.match(/next unchecked step is:\s*"([^"]+)"/i);
  if (m) return m[1];
  if (/propose and start the next loop/i.test(text))
    return "Propose & start next loop";
  const trimmed = text.trim();
  return trimmed.length > 120 ? `${trimmed.slice(0, 117)}…` : trimmed;
}

export type TranscriptItem =
  | { kind: "turn"; turn: ConversationTurn }
  | { kind: "loop-run"; subject: string; count: number; ts: string };

export function groupLoopRuns(turns: ConversationTurn[]): TranscriptItem[] {
  const out: TranscriptItem[] = [];
  let i = 0;
  while (i < turns.length) {
    const turn = turns[i];
    if (turn.role === "user" && turn.source === "loop") {
      const subject = loopPromptSubject(turn.text);
      let count = 1;
      while (
        i + count < turns.length &&
        turns[i + count].role === "user" &&
        turns[i + count].source === "loop" &&
        loopPromptSubject(turns[i + count].text) === subject
      )
        count++;
      out.push({ kind: "loop-run", subject, count, ts: turn.ts });
      i += count;
    } else {
      out.push({ kind: "turn", turn });
      i++;
    }
  }
  return out;
}

export function describeTool(name: string, rawInput: string): string {
  let input: Record<string, unknown> = {};
  try {
    const parsed = JSON.parse(rawInput);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      input = parsed as Record<string, unknown>;
    }
  } catch {
    // Non-JSON tool input falls through to the raw value below.
  }
  const str = (value: unknown): string =>
    typeof value === "string" ? value : JSON.stringify(value);
  const candidate =
    input.file_path ??
    input.path ??
    input.command ??
    input.pattern ??
    input.query ??
    input.url;
  const detail = candidate
    ? str(candidate)
    : rawInput && rawInput !== "{}"
      ? rawInput
      : "";
  return detail ? `${name} · ${detail}` : name;
}

/** Build the allow-rule format understood by Claude Code settings. */
export function buildAllowRule(toolName: string, rawInput: string): string {
  if (toolName.startsWith("mcp__")) return toolName;
  if (toolName === "Bash") {
    let command = "";
    try {
      const parsed = JSON.parse(rawInput);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        const cmd = (parsed as Record<string, unknown>).command;
        if (typeof cmd === "string") command = cmd;
      }
    } catch {
      command = rawInput;
    }
    const firstToken = command.trim().split(/\s+/)[0];
    return firstToken ? `Bash(${firstToken}:*)` : "Bash(*)";
  }
  return `${toolName}(*)`;
}

export function coalesceContentBlocks(blocks: ContentBlock[]): ContentBlock[] {
  const coalesced: ContentBlock[] = [];
  for (const block of blocks) {
    const last = coalesced[coalesced.length - 1];
    if (block.type === "text" && last?.type === "text") {
      coalesced[coalesced.length - 1] = {
        type: "text",
        text: last.text + block.text,
      };
    } else if (block.type === "thinking" && last?.type === "thinking") {
      coalesced[coalesced.length - 1] = {
        type: "thinking",
        thinking: last.thinking + block.thinking,
      };
    } else {
      coalesced.push(block);
    }
  }
  return coalesced;
}
