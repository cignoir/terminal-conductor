import { listen } from "@tauri-apps/api/event";
import { UserAttentionType, getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { useEffect } from "react";
import "@xterm/xterm/css/xterm.css";
import "./App.css";
import { api } from "./api";
import { ImportDialog, ProjectDialog, SettingsDialog } from "./components/Dialogs";
import { ProjectView } from "./components/ProjectView";
import { launchModeFor } from "./components/TerminalPane";
import { QuickSwitcher } from "./components/QuickSwitcher";
import { ConversationSearch } from "./components/ConversationSearch";
import { ContextMenu } from "./components/ContextMenu";
import { Lightbox } from "./components/Lightbox";
import { Tooltip } from "./components/Tooltip";
import { Sidebar } from "./components/Sidebar";
import { setNotifier, useStore } from "./store";
import { startUpdater } from "./updater";
import { parseOsc7501 } from "./terminal/osc7501";
import { resolveLang, useT } from "./i18n";
import { focusTerminal } from "./terminal/cache";
import { type ImageItem, type StatusUpdate, paneId } from "./types";

async function setupNotifications() {
  let granted = await isPermissionGranted().catch(() => false);
  if (!granted) granted = (await requestPermission().catch(() => "denied")) === "granted";
  setNotifier((title, body) => {
    if (granted) sendNotification({ title, body });
    if (!useStore.getState().windowFocused) {
      void getCurrentWindow().requestUserAttention(UserAttentionType.Informational);
    }
  });
}

/** Starts panes of projects marked autoStart without waiting for a view. */
async function autoStart() {
  const s = useStore.getState();
  for (const p of s.projects.filter((x) => x.autoStart)) {
    if (!s.workspace.openProjectIds.includes(p.id)) {
      useStore.setState((st) => ({
        workspace: {
          ...st.workspace,
          openProjectIds: [...st.workspace.openProjectIds, p.id],
        },
      }));
    }
    for (const role of ["claude", "shell"] as const) {
      const id = paneId(p.id, role);
      try {
        const launch = launchModeFor(p, role, s.settings);
        const spawned = await api.ensure({
          paneId: id,
          cwd: p.path,
          shell: p.shell ?? s.settings.defaultShell,
          cols: 120,
          rows: role === "claude" ? 36 : 12,
          launch,
        });
        if (spawned && launch === "resumeOrContinue") {
          s.updateProject(p.id, { continueOnFirstOpen: false });
        }
        s.setAlive(id, true);
      } catch {
        /* shown when the pane is opened */
      }
    }
  }
}

function Notice() {
  const notice = useStore((s) => s.notice);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => useStore.getState().set({ notice: null }), 6000);
    return () => window.clearTimeout(timer);
  }, [notice]);
  if (!notice) return null;
  return (
    <div className="notice" role="status" onClick={() => useStore.getState().set({ notice: null })}>
      {notice}
    </div>
  );
}

export default function App() {
  const t = useT();
  const ready = useStore((s) => s.ready);
  const active = useStore((s) =>
    s.projects.find((p) => p.id === s.workspace.activeProjectId),
  );
  const dialog = useStore((s) => s.dialog);
  const switcherOpen = useStore((s) => s.switcherOpen);
  const hasProjects = useStore((s) => s.projects.length > 0);
  const claudePath = useStore((s) => s.claudePath);

  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];
    unsubs.push(
      listen<StatusUpdate>("claude-status", (e) => useStore.getState().applyStatus(e.payload)),
    );
    unsubs.push(
      listen<{ paneId: string }>("pane-exit", (e) => useStore.getState().markExit(e.payload.paneId)),
    );
    unsubs.push(
      listen<{ paneId: string; payload: string }>("program-status", (e) => {
        const status = parseOsc7501(e.payload.payload);
        if (status) useStore.getState().applyProgramStatus(e.payload.paneId, status);
      }),
    );
    unsubs.push(
      listen<{ paneId: string; items: ImageItem[] }>("images-changed", (e) =>
        useStore.getState().setImages(e.payload.paneId, e.payload.items),
      ),
    );
    void useStore
      .getState()
      .init()
      .then(() => autoStart());
    void setupNotifications();
    startUpdater();

    const onFocus = () => {
      const s = useStore.getState();
      const id = s.workspace.activeProjectId;
      s.set({
        windowFocused: true,
        unread: id ? { ...s.unread, [id]: false } : s.unread,
      });
    };
    const onBlur = () => useStore.getState().set({ windowFocused: false });
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      unsubs.forEach((u) => void u.then((f) => f()));
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  // Global shortcuts. Terminals let these through (see isAppShortcut).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useStore.getState();
      // The image viewer covers everything and handles its own keys; app
      // shortcuts would open dialogs or switch projects hidden behind it.
      if (s.lightbox) return;
      if (e.ctrlKey && !e.altKey && e.key === "Tab") {
        e.preventDefault();
        s.cycle(e.shiftKey ? -1 : 1);
      } else if (e.ctrlKey && !e.altKey && (e.key === "k" || e.key === "K")) {
        e.preventDefault();
        s.set({ switcherOpen: !s.switcherOpen });
      } else if (e.ctrlKey && !e.altKey && !e.shiftKey && /^[1-9]$/.test(e.key)) {
        const id = s.workspace.openProjectIds[Number(e.key) - 1];
        if (id) {
          e.preventDefault();
          s.activate(id);
        }
      } else if (e.altKey && !e.ctrlKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        const id = s.workspace.activeProjectId;
        if (id) {
          e.preventDefault();
          const role = e.key === "ArrowUp" ? "claude" : "shell";
          s.setFocusedRole(id, role);
          focusTerminal(paneId(id, role));
        }
      } else if (e.ctrlKey && e.shiftKey && (e.key === "F" || e.key === "f")) {
        e.preventDefault();
        s.set({ dialog: { kind: "history" }, switcherOpen: false });
      } else if (e.ctrlKey && e.shiftKey && !e.altKey && (e.key === "I" || e.key === "i")) {
        // Also keeps the webview's developer tools shortcut away in dev builds.
        e.preventDefault();
        e.stopPropagation();
        const id = s.workspace.activeProjectId;
        if (id) s.toggleImages(id);
      } else if (e.ctrlKey && e.key === ",") {
        e.preventDefault();
        s.set({ dialog: { kind: "settings" } });
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  useEffect(() => {
    void getCurrentWindow()
      .setTitle(active ? `${active.name} — Terminal Conductor` : "Terminal Conductor")
      .catch(() => {});
  }, [active?.name]);

  const lang = useStore((s) => resolveLang(s.settings.language));
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  if (!ready) return <div className="loading">{t("app.loading")}</div>;

  return (
    <div className="app">
      <Sidebar />
      <main className="main">
        {!claudePath && (
          <div className="warn">
            {t("app.claudeMissing")}
          </div>
        )}
        {active ? (
          <ProjectView key={active.id} project={active} />
        ) : (
          <div className="empty">
            <h2>{hasProjects ? t("empty.pick") : t("empty.welcome")}</h2>
            <p>
              {hasProjects
                ? t("empty.pickBody")
                : t("empty.welcomeBody")}
            </p>
            <div className="empty__actions">
              <button
                className="btn btn--primary"
                onClick={() => useStore.getState().set({ dialog: { kind: "add" } })}
              >
                {t("empty.add")}
              </button>
              <button
                className="btn"
                onClick={() => useStore.getState().set({ dialog: { kind: "import" } })}
              >
                {t("empty.import")}
              </button>
            </div>
            <dl className="keys">
              <dt>Ctrl+K</dt>
              <dd>{t("keys.search")}</dd>
              <dt>Ctrl+Tab</dt>
              <dd>{t("keys.cycle")}</dd>
              <dt>Ctrl+1〜9</dt>
              <dd>{t("keys.nth")}</dd>
              <dt>Alt+↑ / Alt+↓</dt>
              <dd>{t("keys.panes")}</dd>
              <dt>Ctrl+Shift+F</dt>
              <dd>{t("keys.history")}</dd>
              <dt>Ctrl+Shift+I</dt>
              <dd>{t("keys.images")}</dd>
              <dt>Shift+Enter</dt>
              <dd>{t("keys.newline")}</dd>
            </dl>
          </div>
        )}
      </main>
      {switcherOpen && <QuickSwitcher />}
      {dialog?.kind === "add" && <ProjectDialog />}
      {dialog?.kind === "edit" && <ProjectDialog projectId={dialog.projectId} />}
      {dialog?.kind === "import" && <ImportDialog />}
      {dialog?.kind === "settings" && <SettingsDialog />}
      {dialog?.kind === "history" && <ConversationSearch />}
      <Notice />
      <Lightbox />
      <ContextMenu />
      <Tooltip />
    </div>
  );
}
