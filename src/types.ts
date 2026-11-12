export type ShellName = "cmd" | "pwsh" | "powershell";
export type PaneRole = "claude" | "shell";

export type Project = {
  id: string;
  name: string;
  path: string;
  description: string;
  summary?: { text: string; generatedAt: number };
  color: string;
  tags: string[];
  /** Start the panes as soon as the app starts instead of on first view. */
  autoStart: boolean;
  /** Overrides the default shell when set. */
  shell?: ShellName;
  /**
   * On the first open, continue the folder's latest Claude conversation
   * (e.g. one started in another terminal) instead of starting a new one.
   * Cleared after that first attempt.
   */
  continueOnFirstOpen?: boolean;
  /** Height of the Claude pane as a fraction of the project view. */
  splitRatio: number;
  createdAt: number;
};

export type Workspace = {
  openProjectIds: string[];
  activeProjectId?: string;
};

export type Settings = {
  /** "auto" follows the Windows display language. */
  language: "auto" | "en" | "ja";
  fontFamily: string;
  fontSize: number;
  defaultShell: ShellName;
  notifications: boolean;
  /** Start Claude Code in the top pane automatically (off: offer a Resume button). */
  autoStartClaude: boolean;
  /** Conversation search across AI CLIs; off until the user agrees to indexing. */
  historySearch: boolean;
  /** Install a downloaded update when the app is closed normally. */
  installUpdateOnExit: boolean;
  scrollback: number;
};

export type ClaudeStatus =
  | "idle"
  | "working"
  | "needs_permission"
  | "done"
  | "error"
  | "exited";

export type StatusUpdate = {
  paneId: string;
  event: string;
  status: ClaudeStatus | null;
  sessionId: string | null;
  sessionTitle: string | null;
  lastAssistantMessage: string | null;
  notificationType: string | null;
  message: string | null;
};

export type SessionInfo = {
  sessionId?: string;
  sessionTitle?: string;
  lastAssistantMessage?: string;
  lastActivityAt?: number;
};

export type GitInfo = {
  branch: string | null;
  ahead: number;
  behind: number;
  changed: number;
};

/** How Claude Code is started in a pane (see LaunchMode in lib.rs). */
export type LaunchMode = "resume" | "resumeOrContinue" | "continue" | "new";

export type ResumeInfo = { sessionId: string | null; canContinue: boolean };

export type Agent = "claude" | "codex" | "agy";

export type HistoryHit = {
  agent: Agent;
  id: string;
  cwd: string;
  title: string;
  updated: number;
  snippet: string | null;
};

export type HistoryProgress = {
  indexing: boolean;
  done: number;
  total: number;
  conversations: number;
};

export const paneId = (projectId: string, role: PaneRole) =>
  `${projectId}:${role}`;
