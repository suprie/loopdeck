---
name: loopdeck:vite-senior-engineer
description: Use when writing or modifying the Vite + React + TypeScript frontend for the LoopDeck desktop app. Covers component architecture, Zustand state management, Tauri IPC invocation, TypeScript types, and CSS organization.
allowed-tools: [Read, Write, Edit, Glob, Grep, Bash]
---

# Vite Senior Engineer — LoopDeck Frontend

You are a senior frontend engineer working on the LoopDeck desktop app. The stack is Vite 6 + React 19 + TypeScript 5.6 + Zustand 5 + CSS.

## Project Conventions

```
src/
├── main.tsx                      # ReactDOM.createRoot entry
├── App.tsx / App.css             # Root layout + view routing
├── index.css                     # Global CSS variables, resets, theme
├── vite-env.d.ts                 # Vite client types
├── types/
│   └── index.ts                  # All TS types (mirror Rust structs)
├── lib/
│   └── tauri.ts                  # Typed IPC wrappers (NEVER raw invoke())
├── store/
│   └── appStore.ts               # Single Zustand store
├── hooks/
│   ├── useProjects.ts            # Async IPC: scan, import, update, remove
│   └── useToast.ts               # Toast notification queue
└── components/
    ├── layout/
    │   ├── AppShell.tsx/.css     # Window layout shell
    │   └── StatusBar.tsx/.css    # Bottom status bar
    ├── dashboard/
    │   ├── Dashboard.tsx/.css    # Main screen: project list
    │   ├── ProjectRow.tsx/.css   # Single project in list
    │   └── EmptyState.tsx/.css   # "No projects found" state
    ├── import/
    │   ├── ImportFlow.tsx/.css   # Scan results → select → import
    │   └── RepoCard.tsx/.css     # Discovered repo card
    ├── detail/
    │   ├── ProjectDetail.tsx/.css
    │   └── EditDescription.tsx/.css
    └── shared/
        ├── LoadingSpinner.tsx/.css
        └── ConfirmDialog.tsx/.css
```

## TypeScript Types (`src/types/index.ts`)

Mirror Rust structs exactly — use `snake_case` for field names matching serde defaults:

```typescript
export interface DiscoveredRepo {
  path: string;
  name: string;
  markers: string[];          // e.g. ["Cargo.toml", ".git"]
  detected_stack: string;     // e.g. "Rust", "Node.js/TypeScript"
  has_readme: boolean;
  has_loopdeck: boolean;
}

export interface ProjectEntry {
  path: string;
  name: string;
  description: string;
  status: 'active' | 'archived';
  last_opened: string | null;  // ISO 8601
  created_at: string;          // ISO 8601
}

export interface AppError {
  message: string;
  kind: 'config' | 'io' | 'serde' | 'projectNotFound' | 'invalidPath' | 'scan' | 'lockError';
}
```

## IPC Wrappers (`src/lib/tauri.ts`)

NEVER call `invoke()` directly from components. All IPC goes through typed wrappers:

```typescript
import { invoke } from '@tauri-apps/api/core';
import type { DiscoveredRepo, ProjectEntry } from '../types';

export async function scanFolder(
  path: string,
  depth?: number
): Promise<DiscoveredRepo[]> {
  return invoke<DiscoveredRepo[]>('scan_folder', { path, depth });
}

export async function importProject(
  repo: DiscoveredRepo
): Promise<ProjectEntry> {
  return invoke<ProjectEntry>('import_project', { repo });
}

export async function getProjects(): Promise<ProjectEntry[]> {
  return invoke<ProjectEntry[]>('get_projects');
}

export async function getProjectDetail(path: string): Promise<ProjectEntry> {
  return invoke<ProjectEntry>('get_project_detail', { path });
}

export async function updateProject(
  path: string,
  name?: string,
  description?: string
): Promise<ProjectEntry> {
  return invoke<ProjectEntry>('update_project', { path, name, description });
}

export async function removeProject(path: string): Promise<ProjectEntry[]> {
  return invoke<ProjectEntry[]>('remove_project', { path });
}

export async function openRepository(path: string): Promise<void> {
  return invoke<void>('open_repository', { path });
}
```

## Zustand Store (`src/store/appStore.ts`)

Single store with selector-based subscriptions:

```typescript
import { create } from 'zustand';
import type { ProjectEntry, DiscoveredRepo } from '../types';

interface AppState {
  // Data
  projects: ProjectEntry[];
  selectedProject: ProjectEntry | null;
  discoveredRepos: DiscoveredRepo[];

  // UI state
  currentView: 'dashboard' | 'import' | 'detail';
  isScanning: boolean;
  error: string | null;

  // Actions
  setProjects: (projects: ProjectEntry[]) => void;
  setSelectedProject: (project: ProjectEntry | null) => void;
  setDiscoveredRepos: (repos: DiscoveredRepo[]) => void;
  setCurrentView: (view: 'dashboard' | 'import' | 'detail') => void;
  setScanning: (scanning: boolean) => void;
  setError: (error: string | null) => void;
  addProject: (project: ProjectEntry) => void;
  removeProjectByPath: (path: string) => void;
}

export const useAppStore = create<AppState>((set) => ({
  projects: [],
  selectedProject: null,
  discoveredRepos: [],
  currentView: 'dashboard',
  isScanning: false,
  error: null,

  setProjects: (projects) => set({ projects }),
  setSelectedProject: (project) =>
    set({ selectedProject: project, currentView: project ? 'detail' : 'dashboard' }),
  setDiscoveredRepos: (repos) => set({ discoveredRepos: repos }),
  setCurrentView: (view) => set({ currentView: view }),
  setScanning: (scanning) => set({ isScanning: scanning }),
  setError: (error) => set({ error }),
  addProject: (project) =>
    set((state) => ({ projects: [...state.projects, project] })),
  removeProjectByPath: (path) =>
    set((state) => ({
      projects: state.projects.filter((p) => p.path !== path),
    })),
}));
```

## Hook Pattern

Async IPC operations live in hooks, not in the store:

```typescript
// src/hooks/useProjects.ts
import { useCallback } from 'react';
import { useAppStore } from '../store/appStore';
import * as api from '../lib/tauri';
import type { DiscoveredRepo } from '../types';

export function useProjects() {
  const setProjects = useAppStore((s) => s.setProjects);
  const setScanning = useAppStore((s) => s.setScanning);
  const setDiscoveredRepos = useAppStore((s) => s.setDiscoveredRepos);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const setError = useAppStore((s) => s.setError);
  const addProject = useAppStore((s) => s.addProject);
  const removeProjectByPath = useAppStore((s) => s.removeProjectByPath);

  const loadProjects = useCallback(async () => {
    try {
      const projects = await api.getProjects();
      setProjects(projects);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const scan = useCallback(async (path: string, depth?: number) => {
    setScanning(true);
    try {
      const repos = await api.scanFolder(path, depth);
      setDiscoveredRepos(repos);
      setCurrentView('import');
    } catch (err) {
      setError(String(err));
    } finally {
      setScanning(false);
    }
  }, []);

  const importRepo = useCallback(async (repo: DiscoveredRepo) => {
    try {
      const entry = await api.importProject(repo);
      addProject(entry);
      return entry;
    } catch (err) {
      setError(String(err));
      return null;
    }
  }, []);

  const remove = useCallback(async (path: string) => {
    try {
      const projects = await api.removeProject(path);
      setProjects(projects);
      setCurrentView('dashboard');
    } catch (err) {
      setError(String(err));
    }
  }, []);

  return { loadProjects, scan, importRepo, remove };
}
```

Rules:
- Hooks call `useAppStore.getState()` or selector hooks — never mutate store directly
- Every async operation wraps in try/catch, sets error state on failure
- Loading state is set before async call, cleared after (success or failure)
- Hooks return stable function references via `useCallback`

## Component Architecture

### App.tsx — Root + View Router

```typescript
import { useEffect } from 'react';
import { useAppStore } from './store/appStore';
import { useProjects } from './hooks/useProjects';
import { AppShell } from './components/layout/AppShell';
import { Dashboard } from './components/dashboard/Dashboard';
import { ImportFlow } from './components/import/ImportFlow';
import { ProjectDetail } from './components/detail/ProjectDetail';

export default function App() {
  const currentView = useAppStore((s) => s.currentView);
  const { loadProjects } = useProjects();

  useEffect(() => {
    loadProjects();
  }, []);

  return (
    <AppShell>
      {currentView === 'dashboard' && <Dashboard />}
      {currentView === 'import' && <ImportFlow />}
      {currentView === 'detail' && <ProjectDetail />}
    </AppShell>
  );
}
```

### View Switching Pattern
- Use `currentView` in the Zustand store: `'dashboard' | 'import' | 'detail'`
- No React Router needed — this is a desktop app with simple view switching
- `AppShell` provides consistent layout (header, content area, status bar)

### Empty State Pattern
```typescript
// components/dashboard/EmptyState.tsx
export function EmptyState({ onScan }: { onScan: () => void }) {
  return (
    <div className="empty-state">
      <FolderOpenIcon size={64} />
      <h2>No projects found</h2>
      <p>Scan a folder to discover repositories and create project memory.</p>
      <button className="btn-primary" onClick={onScan}>
        Scan Folder
      </button>
    </div>
  );
}
```

### Project Row Pattern
```typescript
// components/dashboard/ProjectRow.tsx
export function ProjectRow({ project, onSelect, onRemove }: Props) {
  return (
    <div className="project-row">
      <div className="project-row__info">
        <h3>{project.name}</h3>
        <p className="project-row__description">{truncate(project.description, 120)}</p>
        <span className="project-row__path">{project.path}</span>
      </div>
      <div className="project-row__actions">
        <button onClick={() => onSelect(project)}>Details</button>
        <button onClick={() => onOpenInFinder(project.path)}>Open</button>
        <button className="btn-danger" onClick={() => onRemove(project.path)}>Remove</button>
      </div>
    </div>
  );
}
```

## Styling Conventions

### CSS Variables (`src/index.css`)

```css
:root {
  /* Colors — dark theme (dev-tool default) */
  --color-bg-primary: #0d1117;
  --color-bg-secondary: #161b22;
  --color-bg-tertiary: #21262d;
  --color-border: #30363d;
  --color-text-primary: #e6edf3;
  --color-text-secondary: #8b949e;
  --color-accent: #58a6ff;
  --color-accent-hover: #79c0ff;
  --color-danger: #f85149;
  --color-success: #3fb950;
  --color-warning: #d2991d;

  /* Spacing */
  --space-xs: 4px;
  --space-sm: 8px;
  --space-md: 16px;
  --space-lg: 24px;
  --space-xl: 32px;

  /* Typography */
  --font-mono: 'SF Mono', 'Fira Code', monospace;
  --font-sans: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  --font-size-sm: 12px;
  --font-size-md: 14px;
  --font-size-lg: 16px;
  --font-size-xl: 20px;

  /* Layout */
  --sidebar-width: 280px;
  --statusbar-height: 32px;
}
```

### Co-located CSS
- Each component has a `.css` file in the same directory
- Use BEM-style naming: `.component-name__element--modifier`
- No CSS modules — plain CSS with BEM is simpler for a small team

### Layout Pattern
```
┌──────────────────────────────────────────────┐
│  AppShell (flex column, full viewport)        │
│  ┌────────────────────────────────────────┐  │
│  │  Header (logo, scan button)            │  │
│  ├────────────────────────────────────────┤  │
│  │  Content (flex-1, scrollable)          │  │
│  │  - Dashboard / ImportFlow / Detail     │  │
│  ├────────────────────────────────────────┤  │
│  │  StatusBar (project count, version)    │  │
│  └────────────────────────────────────────┘  │
└──────────────────────────────────────────────┘
```

## Icons
- Use `lucide-react` for all icons
- Import only needed icons: `import { FolderOpen, RefreshCw, Trash2, Pencil } from 'lucide-react'`

## Error Handling

```typescript
// Catch errors from IPC, show toast or inline error
try {
  const projects = await getProjects();
  setProjects(projects);
} catch (err) {
  // err is AppError { message, kind } from Tauri
  const appError = err as AppError;
  showToast(appError.message, 'error');
}
```

## Performance Rules
- Use Zustand selectors to avoid unnecessary re-renders:
  - `useAppStore((s) => s.projects)` — only re-renders when `projects` changes
  - NOT: `const { projects } = useAppStore()` — re-renders on any state change
- Memoize callbacks in hooks with `useCallback`
- `React.memo()` on list item components (ProjectRow, RepoCard)
- No unnecessary state in components — push to Zustand or hooks

## Vite Configuration (`vite.config.ts`)

```typescript
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,    // Must match tauri.conf.json devUrl
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: ['es2021', 'chrome100', 'safari14'],
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
```

## Build & Run

```bash
# Development
npm run tauri dev

# Type-check (before commits)
npx tsc --noEmit

# Build for production
npm run tauri build
```
