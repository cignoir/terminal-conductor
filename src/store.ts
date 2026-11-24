import { create } from "zustand";
import { api } from "./api";
import { currentLang, t } from "./i18n";
import { type ProgramStatus, statusFromProgram } from "./terminal/osc7501";
import type { MenuState } from "./components/ContextMenu";
import {
  type ClaudeStatus,
  type GitInfo,
  type ImageItem,
  type PaneRole,
  type Project,
  type SessionInfo,
  type Settings,
  type SideTab,
  type StatusUpdate,
  type Workspace,
  paneId,
} from "./types";

export const DEFAULT_SETTINGS: Settings = {
  language: "auto",
  fontFamily:
    "'Cascadia Mono', 'BIZ UDGothic', 'MS Gothic', Consolas, monospace",
  fontSize: 14,
  defaultShell: "cmd",
  notifications: true,
  autoStartClaude: false,
  historySearch: false,
  installUpdateOnExit: true,
  scrollback: 5000,
  imagePanelWidth: 320,
  autoOpenImages: false,
  closeAction: "ask",
};

/** Images found before this time are restored ones, not news. */
const STARTED_AT = Date.now();

export const PROJECT_COLORS = [
  "#5b8def",
  "#3fb27f",
  "#e0a33a",
  "#d9644a",
  "#b072d1",
  "#3bb3c3",
  "#c9c25a",
  "#e06c9f",
  "#8a96a8",
];

export type Dialog =
  | { kind: "add" }
  | { kind: "edit"; projectId: string }
  | { kind: "import" }
  | { kind: "settings" }
  | { kind: "history" }
  | null;

type State = {
  ready: boolean;
  claudePath: string | null;
  dataDir: string;
  projects: Project[];
  workspace: Workspace;
  settings: Settings;
  sessions: Record<string, SessionInfo>;
  status: Record<string, ClaudeStatus>;
  alive: Record<string, boolean>;
  unread: Record<string, boolean>;
  git: Record<string, GitInfo | null>;
  summarizing: Record<string, boolean>;
  focusedRole: Record<string, PaneRole>;
  windowFocused: boolean;
  switcherOpen: boolean;
  dialog: Dialog;
  /** A short message shown at the bottom of the window. */
  notice: string | null;
  contextMenu: MenuState;
  /** Images Claude mentioned, by pane id (newest first). */
  images: Record<string, ImageItem[]>;
  /** New images per project while its panel was closed. */
  unseenImages: Record<string, number>;
  lightbox: { paneId: string; id: string } | null;
  /** The "quit or minimize to the tray?" prompt is open. */
  closePrompt: boolean;
};

type Actions = {
  init: () => Promise<void>;
  addProjects: (items: Partial<Project>[]) => string[];
  updateProject: (id: string, patch: Partial<Project>) => void;
  removeProject: (id: string) => void;
  openProject: (id: string) => void;
  closeProject: (id: string) => void;
  activate: (id: string) => void;
  cycle: (dir: 1 | -1) => void;
  moveOpen: (id: string, dir: 1 | -1) => void;
  applyStatus: (u: StatusUpdate) => void;
  applyProgramStatus: (paneId: string, p: ProgramStatus) => void;
  markExit: (paneId: string) => void;
  setAlive: (paneId: string, alive: boolean) => void;
  refreshGit: (id: string) => Promise<void>;
  summarize: (id: string) => Promise<void>;
  setSettings: (patch: Partial<Settings>) => void;
  setFocusedRole: (projectId: string, role: PaneRole) => void;
  setImages: (paneId: string, items: ImageItem[]) => void;
  toggleImages: (projectId: string, open?: boolean) => void;
  /** Shows a tab of the side panel, or hides the panel if that tab is showing. */
  toggleSideTab: (projectId: string, tab: SideTab) => void;
  /** Free-form notes per project id. */
  notes: Record<string, string>;
  setNote: (projectId: string, text: string) => void;
  set: (patch: Partial<State>) => void;
};

export type Store = State & Actions;

const uid = () => crypto.randomUUID();

function normalizeProject(p: Partial<Project>, i: number): Project {
  return {
    id: p.id ?? uid(),
    name: p.name ?? folderName(p.path ?? "") ?? "project",
    path: p.path ?? "",
    description: p.description ?? "",
    summary: p.summary,
    color: p.color ?? PROJECT_COLORS[i % PROJECT_COLORS.length],
    tags: p.tags ?? [],
    autoStart: p.autoStart ?? false,
    shell: p.shell,
    shellDir: p.shellDir,
    continueOnFirstOpen: p.continueOnFirstOpen,
    splitRatio: p.splitRatio ?? 0.72,
    imagesOpen: p.imagesOpen ?? false,
    sideTab: p.sideTab,
    createdAt: p.createdAt ?? Date.now(),
  };
}

/** Resolves the folder the bottom pane's shell starts in. */
export function shellDirOf(project: Pick<Project, "path" | "shellDir">): string {
  const dir = project.shellDir?.trim();
  if (!dir) return project.path;
  if (/^[a-zA-Z]:[\\/]/.test(dir) || /^[\\/]/.test(dir)) return dir;
  return `${project.path.replace(/[\\/]+$/, "")}\\${dir.replace(/^\.[\\/]/, "")}`;
}

/** The folder a pane starts in; falls back to the project folder if gone. */
export async function paneCwd(project: Project, role: "claude" | "shell"): Promise<string> {
  if (role === "claude" || !project.shellDir?.trim()) return project.path;
  const dir = shellDirOf(project);
  return (await api.pathExists(dir).catch(() => false)) ? dir : project.path;
}

export function folderName(path: string): string {
  const parts = path.replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

const STATUS_RANK: Record<ClaudeStatus, number> = {
  needs_permission: 6,
  error: 5,
  done: 4,
  working: 3,
  idle: 2,
  exited: 1,
};

/** The most attention-worthy status across a project's panes. */
export function projectStatus(
  s: Pick<State, "status">,
  projectId: string,
): ClaudeStatus | undefined {
  let best: ClaudeStatus | undefined;
  for (const role of ["claude", "shell"] as PaneRole[]) {
    const st = s.status[paneId(projectId, role)];
    if (st && (!best || STATUS_RANK[st] > STATUS_RANK[best])) best = st;
  }
  return best;
}

export function projectIdOfPane(pid: string): string {
  return pid.slice(0, pid.lastIndexOf(":"));
}

export function lastActivity(
  s: Pick<State, "sessions">,
  projectId: string,
): number | undefined {
  const a = s.sessions[paneId(projectId, "claude")]?.lastActivityAt;
  const b = s.sessions[paneId(projectId, "shell")]?.lastActivityAt;
  if (a === undefined) return b;
  if (b === undefined) return a;
  return Math.max(a, b);
}

export function lastMessage(
  s: Pick<State, "sessions">,
  projectId: string,
): string | undefined {
  const c = s.sessions[paneId(projectId, "claude")];
  const sh = s.sessions[paneId(projectId, "shell")];
  if (!sh?.lastAssistantMessage) return c?.lastAssistantMessage;
  if (!c?.lastAssistantMessage) return sh.lastAssistantMessage;
  return (c.lastActivityAt ?? 0) >= (sh.lastActivityAt ?? 0)
    ? c.lastAssistantMessage
    : sh.lastAssistantMessage;
}

type Notify = (title: string, body: string) => void;
let notifier: Notify = () => {};
export function setNotifier(fn: Notify) {
  notifier = fn;
}

export const useStore = create<Store>((set, get) => ({
  ready: false,
  claudePath: null,
  dataDir: "",
  projects: [],
  workspace: { openProjectIds: [] },
  settings: DEFAULT_SETTINGS,
  sessions: {},
  status: {},
  alive: {},
  unread: {},
  git: {},
  summarizing: {},
  focusedRole: {},
  windowFocused: true,
  switcherOpen: false,
  dialog: null,
  notice: null,
  contextMenu: null,
  images: {},
  unseenImages: {},
  notes: {},
  lightbox: null,
  closePrompt: false,

  set: (patch) => set(patch),

  init: async () => {
    const d = await api.init();
    const rawProjects = (d.projects as { projects?: Partial<Project>[] } | null)
      ?.projects;
    const projects = (rawProjects ?? []).map(normalizeProject);
    const ids = new Set(projects.map((p) => p.id));
    const ws = (d.workspace as Workspace | null) ?? { openProjectIds: [] };
    const workspace: Workspace = {
      openProjectIds: (ws.openProjectIds ?? []).filter((id) => ids.has(id)),
      activeProjectId:
        ws.activeProjectId && ids.has(ws.activeProjectId)
          ? ws.activeProjectId
          : undefined,
    };
    if (!workspace.activeProjectId && workspace.openProjectIds.length) {
      workspace.activeProjectId = workspace.openProjectIds[0];
    }
    const alive = Object.fromEntries((await api.alive()).map((p) => [p, true]));
    set({
      ready: true,
      claudePath: d.claudePath,
      dataDir: d.dataDir,
      projects,
      workspace,
      settings: { ...DEFAULT_SETTINGS, ...((d.settings as object) ?? {}) },
      sessions: d.sessions ?? {},
      notes: readNotes(d.notes),
      alive,
    });
  },

  addProjects: (items) => {
    const existing = get().projects;
    const known = new Set(existing.map((p) => p.path.toLowerCase()));
    const fresh = items
      .filter((p) => p.path && !known.has(p.path.toLowerCase()))
      .map((p, i) => normalizeProject(p, existing.length + i));
    if (fresh.length) set({ projects: [...existing, ...fresh] });
    return fresh.map((p) => p.id);
  },

  updateProject: (id, patch) =>
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? { ...p, ...patch } : p)),
    })),

  removeProject: (id) => {
    const panes = (["claude", "shell"] as PaneRole[]).map((r) => paneId(id, r));
    get().closeProject(id);
    set((s) => {
      const { [id]: _removed, ...notes } = s.notes;
      return { projects: s.projects.filter((p) => p.id !== id), notes };
    });
    // closeProject stops the panes gracefully; drop their state afterwards.
    void Promise.all(panes.map((p) => api.kill(p))).then(() =>
      panes.forEach((p) => void api.forget(p)),
    );
  },

  openProject: (id) => {
    const ws = get().workspace;
    const open = ws.openProjectIds.includes(id)
      ? ws.openProjectIds
      : [...ws.openProjectIds, id];
    set((s) => ({
      workspace: { openProjectIds: open, activeProjectId: id },
      unread: { ...s.unread, [id]: false },
    }));
  },

  closeProject: (id) => {
    for (const role of ["claude", "shell"] as PaneRole[]) {
      void api.kill(paneId(id, role));
    }
    set((s) => {
      const open = s.workspace.openProjectIds.filter((x) => x !== id);
      let active = s.workspace.activeProjectId;
      if (active === id) {
        const idx = s.workspace.openProjectIds.indexOf(id);
        active = open[Math.min(idx, open.length - 1)];
      }
      return { workspace: { openProjectIds: open, activeProjectId: active } };
    });
  },

  activate: (id) => get().openProject(id),

  cycle: (dir) => {
    const { openProjectIds, activeProjectId } = get().workspace;
    if (!openProjectIds.length) return;
    const i = activeProjectId ? openProjectIds.indexOf(activeProjectId) : -1;
    const next =
      openProjectIds[(i + dir + openProjectIds.length) % openProjectIds.length];
    get().activate(next);
  },

  moveOpen: (id, dir) =>
    set((s) => {
      const open = [...s.workspace.openProjectIds];
      const i = open.indexOf(id);
      const j = i + dir;
      if (i < 0 || j < 0 || j >= open.length) return {};
      [open[i], open[j]] = [open[j], open[i]];
      return { workspace: { ...s.workspace, openProjectIds: open } };
    }),

  applyStatus: (u) => {
    const s = get();
    const sessions = { ...s.sessions };
    const prev = sessions[u.paneId] ?? {};
    sessions[u.paneId] = {
      ...prev,
      sessionId: u.sessionId ?? prev.sessionId,
      sessionTitle: u.sessionTitle ?? prev.sessionTitle,
      lastAssistantMessage: u.lastAssistantMessage ?? prev.lastAssistantMessage,
      lastActivityAt: Date.now(),
    };
    const patch: Partial<State> = { sessions };
    const projectId = projectIdOfPane(u.paneId);
    if (u.status) {
      const before = s.status[u.paneId];
      patch.status = { ...s.status, [u.paneId]: u.status };
      const visible =
        s.workspace.activeProjectId === projectId && s.windowFocused;
      const attention =
        (u.status === "needs_permission" || u.status === "done" ||
          u.status === "error") &&
        before !== u.status;
      if (attention && !visible) {
        patch.unread = { ...s.unread, [projectId]: true };
        const project = s.projects.find((p) => p.id === projectId);
        if (project && s.settings.notifications) {
          const body =
            u.status === "needs_permission"
              ? (u.message ?? t("notify.permission"))
              : u.status === "error"
                ? t("notify.error")
                : (u.lastAssistantMessage?.split("\n")[0] ?? t("notify.done"));
          notifier(project.name, body);
        }
      }
    }
    set(patch);
  },

  applyProgramStatus: (pid, p) => {
    const status = statusFromProgram(p);
    if (!status) return;
    // "idle" follows "done"/"error" right away; keep showing the result until
    // the next turn starts.
    const current = get().status[pid];
    if (status === "idle" && (current === "done" || current === "error")) return;
    if (status === current && status !== "done") return;
    get().applyStatus({
      paneId: pid,
      event: `osc7501:${p.state}`,
      status,
      sessionId: null,
      sessionTitle: null,
      lastAssistantMessage: status === "done" ? (p.msg ?? null) : null,
      notificationType: p.kind ?? null,
      message: status === "needs_permission" ? (p.msg ?? null) : null,
    });
  },

  markExit: (pid) =>
    set((s) => ({
      alive: { ...s.alive, [pid]: false },
      status: s.status[pid]
        ? { ...s.status, [pid]: "exited" as ClaudeStatus }
        : s.status,
    })),

  setAlive: (pid, alive) => set((s) => ({ alive: { ...s.alive, [pid]: alive } })),

  refreshGit: async (id) => {
    const p = get().projects.find((x) => x.id === id);
    if (!p) return;
    const info = await api.gitStatus(p.path).catch(() => null);
    set((s) => ({ git: { ...s.git, [id]: info } }));
  },

  summarize: async (id) => {
    const p = get().projects.find((x) => x.id === id);
    if (!p || get().summarizing[id]) return;
    set((s) => ({ summarizing: { ...s.summarizing, [id]: true } }));
    try {
      const text = await api.generateSummary(p.path, currentLang());
      get().updateProject(id, { summary: { text, generatedAt: Date.now() } });
    } finally {
      set((s) => ({ summarizing: { ...s.summarizing, [id]: false } }));
    }
  },

  setSettings: (patch) => set((s) => ({ settings: { ...s.settings, ...patch } })),

  setFocusedRole: (projectId, role) =>
    set((s) => ({ focusedRole: { ...s.focusedRole, [projectId]: role } })),

  setImages: (pid, items) => {
    const s = get();
    const before = new Map((s.images[pid] ?? []).map((i) => [i.id, i.modified]));
    const fresh = items.filter(
      (i) => i.foundAt >= STARTED_AT && before.get(i.id) !== i.modified,
    ).length;
    const patch: Partial<State> = { images: { ...s.images, [pid]: items } };
    const projectId = projectIdOfPane(pid);
    const project = s.projects.find((p) => p.id === projectId);
    if (fresh && project && !project.imagesOpen) {
      if (s.settings.autoOpenImages) {
        get().toggleImages(projectId, true);
      } else {
        patch.unseenImages = {
          ...s.unseenImages,
          [projectId]: (s.unseenImages[projectId] ?? 0) + fresh,
        };
      }
    }
    if (s.lightbox?.paneId === pid && !items.some((i) => i.id === s.lightbox?.id)) {
      patch.lightbox = null;
    }
    set(patch);
  },

  toggleImages: (projectId, open) => {
    const p = get().projects.find((x) => x.id === projectId);
    if (!p) return;
    const next = open ?? !p.imagesOpen;
    get().updateProject(projectId, { imagesOpen: next });
    if (next) set((s) => ({ unseenImages: { ...s.unseenImages, [projectId]: 0 } }));
  },

  toggleSideTab: (projectId, tab) => {
    const p = get().projects.find((x) => x.id === projectId);
    if (!p) return;
    const showing = p.imagesOpen && (p.sideTab ?? "media") === tab;
    get().updateProject(projectId, { imagesOpen: !showing, sideTab: tab });
    if (!showing && tab === "media") {
      set((s) => ({ unseenImages: { ...s.unseenImages, [projectId]: 0 } }));
    }
  },

  setNote: (projectId, text) =>
    set((s) => {
      const notes = { ...s.notes };
      if (text) notes[projectId] = text;
      else delete notes[projectId];
      return { notes };
    }),
}));

const docTimers: Record<string, number | undefined> = {};

/** Writes all documents now, skipping the debounce. Used before an update. */
export async function flushDocuments() {
  for (const t of Object.values(docTimers)) window.clearTimeout(t);
  const s = useStore.getState();
  if (!s.ready) return;
  await Promise.all([
    api.saveDoc("projects", { version: 1, projects: s.projects }),
    api.saveDoc("workspace", s.workspace),
    api.saveDoc("settings", s.settings),
    api.saveDoc("notes", { version: 1, notes: s.notes }),
  ]);
}

// Persist documents shortly after they change.
function persist<T>(
  select: (s: Store) => T,
  name: "projects" | "workspace" | "settings" | "notes",
  wrap: (v: T) => unknown,
) {
  useStore.subscribe((s, prev) => {
    if (!s.ready || select(s) === select(prev)) return;
    window.clearTimeout(docTimers[name]);
    docTimers[name] = window.setTimeout(() => {
      docTimers[name] = undefined;
      void api.saveDoc(name, wrap(select(useStore.getState())));
    }, 300);
  });
}
persist((s) => s.projects, "projects", (projects) => ({ version: 1, projects }));
persist((s) => s.workspace, "workspace", (w) => w);
persist((s) => s.settings, "settings", (v) => v);
persist((s) => s.notes, "notes", (notes) => ({ version: 1, notes }));

function readNotes(doc: unknown): Record<string, string> {
  const notes = (doc as { notes?: unknown } | null)?.notes;
  if (!notes || typeof notes !== "object") return {};
  return Object.fromEntries(
    Object.entries(notes as Record<string, unknown>).filter(
      (e): e is [string, string] => typeof e[1] === "string" && e[1] !== "",
    ),
  );
}
