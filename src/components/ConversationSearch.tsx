import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openPath } from "@tauri-apps/plugin-opener";
import { Fragment, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { confirmDialog } from "../confirm";
import { t as translate, useT } from "../i18n";
import { folderName, useStore } from "../store";
import { getTerminal } from "../terminal/cache";
import { type Agent, type HistoryHit, type HistoryProgress, paneId } from "../types";
import { relativeTime } from "../util";

export const AGENT_LABEL: Record<Agent, string> = {
  claude: "Claude",
  codex: "Codex",
  agy: "agy",
};

export function resumeCommand(hit: HistoryHit): string {
  switch (hit.agent) {
    case "claude":
      return `claude --resume ${hit.id}`;
    case "codex":
      return `codex resume ${hit.id}`;
    case "agy":
      return `agy --conversation ${hit.id}`;
  }
}

const samePath = (a: string, b: string) =>
  a.replace(/[\\/]+$/, "").toLowerCase() === b.replace(/[\\/]+$/, "").toLowerCase();

async function waitForAlive(id: string, ms = 8000) {
  const start = Date.now();
  while (Date.now() - start < ms) {
    if (useStore.getState().alive[id]) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
}

/**
 * Opens the conversation's folder as a project (adding it if needed) and
 * resumes the conversation in the top pane. If that pane was already
 * running, something may be using it, so ask before typing into it.
 */
export async function openConversation(hit: HistoryHit): Promise<string | null> {
  const s = useStore.getState();
  let project = s.projects.find((p) => samePath(p.path, hit.cwd));
  if (!project) {
    if (!(await api.pathExists(hit.cwd))) return translate("history.folderMissing", { path: hit.cwd });
    const [id] = s.addProjects([{ path: hit.cwd, name: folderName(hit.cwd) }]);
    project = useStore.getState().projects.find((p) => p.id === id);
    if (!project) return translate("history.folderMissing", { path: hit.cwd });
  }
  const pane = paneId(project.id, "claude");
  const wasRunning = !!useStore.getState().alive[pane];
  const autoStart = useStore.getState().settings.autoStartClaude;

  // With automatic start, a pane that starts now runs Claude by itself. For
  // a Claude conversation, make it the pane's session so that start resumes
  // it; anything else would be typed into Claude, so treat the pane as busy.
  if (!wasRunning && autoStart && hit.agent === "claude") {
    await api.conversationResume(pane, hit.agent, hit.id, false);
    s.activate(project.id);
    s.setFocusedRole(project.id, "claude");
    return null;
  }

  s.activate(project.id);
  s.setFocusedRole(project.id, "claude");
  if (!(await waitForAlive(pane))) return translate("history.paneNotReady");

  const command = resumeCommand(hit);
  const occupied = wasRunning || autoStart;
  const busy =
    occupied && !(await confirmDialog(translate("history.confirmType", { command })));
  if (busy) {
    await writeText(command);
    return translate("history.copied", { command });
  }
  await api.conversationResume(pane, hit.agent, hit.id);
  window.dispatchEvent(new CustomEvent("tc-offer-dismiss", { detail: pane }));
  getTerminal(pane, useStore.getState().settings).term.focus();
  return null;
}

/** Splits text around case-insensitive matches of the terms, for highlighting. */
function highlight(text: string, terms: string[]) {
  const words = terms.filter(Boolean).map((w) => w.toLowerCase());
  if (!words.length) return text;
  const lower = text.toLowerCase();
  const parts: { s: string; hit: boolean }[] = [];
  let i = 0;
  while (i < text.length) {
    let best = -1;
    let len = 0;
    for (const w of words) {
      const at = lower.indexOf(w, i);
      if (at >= 0 && (best < 0 || at < best)) {
        best = at;
        len = w.length;
      }
    }
    if (best < 0) {
      parts.push({ s: text.slice(i), hit: false });
      break;
    }
    if (best > i) parts.push({ s: text.slice(i, best), hit: false });
    parts.push({ s: text.slice(best, best + len), hit: true });
    i = best + len;
  }
  return parts.map((p, k) =>
    p.hit ? <mark key={k}>{p.s}</mark> : <Fragment key={k}>{p.s}</Fragment>,
  );
}

export function ConversationSearch() {
  const t = useT();
  const enabled = useStore((s) => s.settings.historySearch);
  const setSettings = useStore((s) => s.setSettings);
  const set = useStore((s) => s.set);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<HistoryHit[]>([]);
  const [sel, setSel] = useState(0);
  const [progress, setProgress] = useState<HistoryProgress | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const close = () => set({ dialog: null });

  // Refresh the index whenever the dialog opens (only changed files are read).
  useEffect(() => {
    if (!enabled) return;
    const unlisten = listen<HistoryProgress>("history-progress", (e) => setProgress(e.payload));
    void api.historyRefresh().then(setProgress).catch((e) => setMessage(String(e)));
    inputRef.current?.focus();
    return () => void unlisten.then((f) => f());
  }, [enabled]);

  // Search as the user types, and again as indexing makes progress.
  useEffect(() => {
    if (!enabled) return;
    const timer = window.setTimeout(() => {
      api
        .historySearch(query, 200)
        .then((h) => {
          setHits(h);
          setSel((i) => Math.min(i, Math.max(0, h.length - 1)));
        })
        .catch((e) => setMessage(String(e)));
    }, 150);
    return () => window.clearTimeout(timer);
  }, [query, enabled, progress?.done, progress?.indexing]);

  const choose = async (hit: HistoryHit | undefined) => {
    if (!hit) return;
    close();
    const note = await openConversation(hit);
    if (note) useStore.getState().set({ notice: note });
  };

  if (!enabled) {
    return (
      <div className="overlay" onMouseDown={close}>
        <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
          <h2 className="modal__title">{t("history.title")}</h2>
          <div className="modal__body">
            <p>{t("history.consentIntro")}</p>
            <ul className="consent">
              <li>{t("history.consentSources")}</li>
              <li>{t("history.consentCopy")}</li>
              <li>{t("history.consentLocal")}</li>
              <li>{t("history.consentDelete")}</li>
            </ul>
          </div>
          <div className="modal__footer">
            <span className="spacer" />
            <button className="btn btn--ghost" onClick={close}>
              {t("dialog.cancel")}
            </button>
            <button className="btn btn--primary" onClick={() => setSettings({ historySearch: true })}>
              {t("history.enable")}
            </button>
          </div>
        </div>
      </div>
    );
  }

  const terms = query.split(/\s+/).filter(Boolean);
  const status = progress?.indexing
    ? t("history.indexing", { done: progress.done, total: progress.total })
    : progress
      ? t("history.indexed", { count: progress.conversations })
      : "";

  return (
    <div className="overlay" onMouseDown={close}>
      <div className="switcher switcher--wide" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          value={query}
          placeholder={t("history.placeholder")}
          spellCheck={false}
          onChange={(e) => {
            setQuery(e.target.value);
            setSel(0);
          }}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (e.key === "Escape") close();
            else if (e.key === "ArrowDown") {
              e.preventDefault();
              setSel((i) => Math.min(i + 1, hits.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSel((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter") {
              void choose(hits[sel]);
            }
          }}
        />
        <div className="switcher__status">
          <span>{status}</span>
          {message && <span className="error">{message}</span>}
        </div>
        <ul className="switcher__list history">
          {hits.map((h, i) => (
            <li
              key={`${h.agent}:${h.id}`}
              className={i === sel ? "is-selected" : ""}
              onMouseEnter={() => setSel(i)}
              onClick={() => void choose(h)}
            >
              <div className="history__head">
                <span className={`agent agent--${h.agent}`}>{AGENT_LABEL[h.agent]}</span>
                <span className="history__title">{highlight(h.title || h.id, terms)}</span>
                <span className="row__time">{relativeTime(h.updated, t)}</span>
              </div>
              <div className="history__path">{highlight(h.cwd, terms)}</div>
              {h.snippet && <div className="history__snippet">{highlight(h.snippet, terms)}</div>}
              <div className="history__tools" onClick={(e) => e.stopPropagation()}>
                <button
                  className="btn btn--ghost btn--small"
                  onClick={() => {
                    void writeText(resumeCommand(h));
                    setMessage(t("history.copied", { command: resumeCommand(h) }));
                  }}
                >
                  {t("history.copyCommand")}
                </button>
                <button className="btn btn--ghost btn--small" onClick={() => void openPath(h.cwd)}>
                  {t("project.openFolder")}
                </button>
              </div>
            </li>
          ))}
          {hits.length === 0 && !progress?.indexing && (
            <li className="muted">{t("switcher.none")}</li>
          )}
        </ul>
      </div>
    </div>
  );
}
