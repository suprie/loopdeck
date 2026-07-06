export type ProjectStatus = "active" | "warning" | "inactive" | "archived";
export type RunState = "idle" | "working" | "waiting" | "done";

export interface Project {
  id: string;
  name: string;
  monogram: string;
  path: string;
  description: string;
  status: ProjectStatus;
  openedAgo: string;
  lastCommit: { message: string; ago: string };
  lastModified: string;
  currentLoop?: string;
  createdAt: string;
  accentFrom: string;
  accentTo: string;
  runState: RunState;
  uncommitted: { files: number; added: number; deleted: number };
}


export const projects: Project[] = [
  {
    id: "loopdeck",
    name: "loopdeck",
    monogram: "L",
    path: "/Users/suprie/Workspace/others/loopdeck",
    description:
      "Local-first desktop app for structured project memory — stored right inside your repo.",
    status: "active",
    openedAgo: "2h ago",
    lastCommit: { message: "feat: add streaming agent chat", ago: "3h ago" },
    lastModified: "1h ago",
    currentLoop: "Refactor chat UI",
    createdAt: "Jun 22",
    accentFrom: "oklch(0.7 0.19 295)",
    accentTo: "oklch(0.72 0.18 340)",
    runState: "working",
    uncommitted: { files: 12, added: 340, deleted: 58 }
  },
  {
    id: "agentui",
    name: "agentui",
    monogram: "A",
    path: "/Users/suprie/Workspace/labs/agentui",
    description:
      "Composable React primitives for building agent chat surfaces with tool-call rendering.",
    status: "active",
    openedAgo: "1d ago",
    lastCommit: { message: "chore: bump react to 19.1", ago: "6h ago" },
    lastModified: "4h ago",
    currentLoop: "Tool call accordion",
    createdAt: "May 3",
    accentFrom: "oklch(0.72 0.18 200)",
    accentTo: "oklch(0.75 0.17 160)",
    runState: "waiting",
    uncommitted: { files: 3, added: 42, deleted: 17 }
  },
  {
    id: "foobar",
    name: "foobar",
    monogram: "F",
    path: "/Users/suprie/Workspace/scratch/foobar",
    description: "Playground for testing DSP experiments and audio latency measurements.",
    status: "inactive",
    openedAgo: "3w ago",
    lastCommit: { message: "wip: sketch resampler", ago: "3w ago" },
    lastModified: "3w ago",
    createdAt: "Apr 12",
    accentFrom: "oklch(0.72 0.16 60)",
    accentTo: "oklch(0.75 0.18 30)",
    runState: "idle",
    uncommitted: { files: 0, added: 0, deleted: 0 }
  },
  {
    id: "harbor",
    name: "harbor",
    monogram: "H",
    path: "/Users/suprie/Workspace/clients/harbor",
    description: "Internal ops dashboard for tracking freight lanes and dispatcher decisions.",
    status: "warning",
    openedAgo: "5h ago",
    lastCommit: { message: "fix: dispatcher rounding drift", ago: "5h ago" },
    lastModified: "5h ago",
    currentLoop: "Investigate rounding bug",
    createdAt: "Feb 1",
    accentFrom: "oklch(0.75 0.15 75)",
    accentTo: "oklch(0.72 0.19 25)",
    runState: "working",
    uncommitted: { files: 7, added: 128, deleted: 214 }
  },
  {
    id: "notebook",
    name: "notebook",
    monogram: "N",
    path: "/Users/suprie/Workspace/personal/notebook",
    description:
      "Personal writing archive with weekly review prompts and structured captures.",
    status: "active",
    openedAgo: "8h ago",
    lastCommit: { message: "post: june retrospective", ago: "2d ago" },
    lastModified: "8h ago",
    currentLoop: "Weekly review",
    createdAt: "Jan 8",
    accentFrom: "oklch(0.72 0.16 155)",
    accentTo: "oklch(0.74 0.14 200)",
    runState: "done",
    uncommitted: { files: 1, added: 22, deleted: 4 }
  },
  {
    id: "atlas",
    name: "atlas",
    monogram: "A",
    path: "/Users/suprie/Workspace/archive/atlas",
    description: "Legacy geo indexing pipeline. Archived after migration to titan.",
    status: "archived",
    openedAgo: "2mo ago",
    lastCommit: { message: "chore: freeze", ago: "2mo ago" },
    lastModified: "2mo ago",
    createdAt: "Oct 4 2024",
    accentFrom: "oklch(0.65 0.02 270)",
    accentTo: "oklch(0.7 0.02 270)",
    runState: "idle",
    uncommitted: { files: 0, added: 0, deleted: 0 }
  },
];

export function getProject(id: string) {
  return projects.find((p) => p.id === id);
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content?: string;
  thinking?: string;
  toolCalls?: { kind: string; detail: string }[];
  meta?: { timeSec: number; tokensIn: number; tokensOut: number; costUsd: number };
  approval?: {
    title: string;
    label: string;
    detail: string;
  };
}

export const sampleChat: ChatMessage[] = [
  {
    id: "u1",
    role: "user",
    content: "What's the current loop for this project?",
  },
  {
    id: "a1",
    role: "assistant",
    thinking:
      "The user is asking about the current loop. I should check loops.md and cross-reference the active loop metadata.",
    toolCalls: [
      { kind: "Read", detail: "src/components/detail/Chat.tsx" },
      { kind: "Grep", detail: 'pattern="useAppStore"' },
    ],
    content:
      'The current loop is "Refactor chat UI". Based on loops.md, the next step is to extract the BlockList component into its own module so streaming tool calls can be rendered independently from the message body.',
    meta: { timeSec: 4.2, tokensIn: 1204, tokensOut: 856, costUsd: 0.0124 },
  },
  {
    id: "a2",
    role: "assistant",
    approval: {
      title: "The agent needs your approval",
      label: "EDIT",
      detail: "Edit · src/components/detail/Chat.tsx",
    },
  },
];
