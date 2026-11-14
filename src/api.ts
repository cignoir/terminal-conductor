import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  Agent,
  GitInfo,
  HistoryHit,
  HistoryProgress,
  ImageItem,
  LaunchMode,
  ResumeInfo,
  SessionInfo,
  ShellName,
} from "./types";

export type InitData = {
  dataDir: string;
  claudePath: string | null;
  projects: unknown;
  workspace: unknown;
  settings: unknown;
  sessions: Record<string, SessionInfo>;
};

export type AttachResult = { attachId: number; replayed: number; alive: boolean };

export const api = {
  init: () => invoke<InitData>("app_init"),
  saveDoc: (name: "projects" | "workspace" | "settings", value: unknown) =>
    invoke<void>("save_doc", { name, value }),
  ensure: (args: {
    paneId: string;
    cwd: string;
    shell: ShellName;
    cols: number;
    rows: number;
    /** Start Claude Code right after the shell; omit to start only the shell. */
    launch?: LaunchMode;
  }) => invoke<boolean>("pane_ensure", { args }),
  claudeLaunch: (paneId: string, cwd: string, mode: LaunchMode) =>
    invoke<void>("claude_launch", { paneId, cwd, mode }),
  claudeResumeInfo: (paneId: string, cwd: string) =>
    invoke<ResumeInfo>("claude_resume_info", { paneId, cwd }),
  attach: (paneId: string, channel: Channel<ArrayBuffer>) =>
    invoke<AttachResult>("pane_attach", { paneId, channel }),
  detach: (paneId: string, attachId: number) =>
    invoke<void>("pane_detach", { paneId, attachId }),
  write: (paneId: string, data: string) =>
    invoke<void>("pane_write", { paneId, data }),
  resize: (paneId: string, cols: number, rows: number) =>
    invoke<void>("pane_resize", { paneId, cols, rows }),
  kill: (paneId: string) => invoke<void>("pane_kill", { paneId }),
  forget: (paneId: string) => invoke<void>("pane_forget", { paneId }),
  alive: () => invoke<string[]>("panes_alive"),
  gitStatus: (path: string) => invoke<GitInfo | null>("git_status", { path }),
  scanProjects: (root: string) => invoke<string[]>("scan_projects", { root }),
  generateSummary: (path: string, lang: string) =>
    invoke<string>("generate_summary", { path, lang }),
  historyRefresh: () => invoke<HistoryProgress>("history_refresh"),
  historySearch: (query: string, limit: number) =>
    invoke<HistoryHit[]>("history_search", { query, limit }),
  historyDelete: () => invoke<void>("history_delete"),
  conversationResume: (paneId: string, agent: Agent, id: string, typeLine = true) =>
    invoke<void>("conversation_resume", { paneId, agent, id, typeLine }),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
  imagesList: (paneId: string) => invoke<ImageItem[]>("images_list", { paneId }),
  imagesRemove: (paneId: string, id: string) =>
    invoke<ImageItem[]>("images_remove", { paneId, id }),
  imagesClear: (paneId: string) => invoke<void>("images_clear", { paneId }),
  imageCopy: (id: string) => invoke<void>("image_copy", { id }),
};

/** Where the app's image protocol (tcimg) serves a listed image. */
export function imageUrl(item: ImageItem, kind: "thumb" | "full") {
  return `http://tcimg.localhost/${item.id}/${kind}?v=${item.modified}`;
}
