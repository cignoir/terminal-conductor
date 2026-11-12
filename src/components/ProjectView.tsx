import { openPath } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { lastMessage, projectStatus, useStore } from "../store";
import { type Project, paneId } from "../types";
import { StatusBadge } from "./StatusBadge";
import { TerminalPane, requestRestart } from "./TerminalPane";
import { relativeTime } from "../util";
import { confirmDialog } from "../confirm";
import { useT } from "../i18n";

export function ProjectView({ project }: { project: Project }) {
  const t = useT();
  const focusedRole = useStore((s) => s.focusedRole[project.id] ?? "claude");
  const setFocusedRole = useStore((s) => s.setFocusedRole);
  const updateProject = useStore((s) => s.updateProject);
  const containerRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    if (!dragging) return;
    const move = (e: PointerEvent) => {
      const el = containerRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const ratio = (e.clientY - rect.top) / rect.height;
      updateProject(project.id, {
        splitRatio: Math.min(0.92, Math.max(0.2, ratio)),
      });
    };
    const up = () => setDragging(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [dragging, project.id, updateProject]);

  return (
    <div className="project">
      <ProjectHeader project={project} />
      <div
        className={`split ${dragging ? "split--dragging" : ""}`}
        ref={containerRef}
        style={{
          gridTemplateRows: `${project.splitRatio}fr 6px ${1 - project.splitRatio}fr`,
        }}
      >
        <TerminalPane
          project={project}
          role="claude"
          focused={focusedRole === "claude"}
          onFocus={() => setFocusedRole(project.id, "claude")}
        />
        <div
          className="split__handle"
          onPointerDown={(e) => {
            e.preventDefault();
            setDragging(true);
          }}
          onDoubleClick={() => updateProject(project.id, { splitRatio: 0.72 })}
          title={t("project.resizeHandle")}
        />
        <TerminalPane
          project={project}
          role="shell"
          focused={focusedRole === "shell"}
          onFocus={() => setFocusedRole(project.id, "shell")}
        />
      </div>
    </div>
  );
}

function ProjectHeader({ project }: { project: Project }) {
  const t = useT();
  const git = useStore((s) => s.git[project.id]);
  const status = useStore((s) => projectStatus(s, project.id));
  const message = useStore((s) => lastMessage(s, project.id));
  const title = useStore((s) => s.sessions[paneId(project.id, "claude")]?.sessionTitle);
  const summarizing = useStore((s) => s.summarizing[project.id]);
  const refreshGit = useStore((s) => s.refreshGit);
  const summarize = useStore((s) => s.summarize);
  const set = useStore((s) => s.set);
  const [infoOpen, setInfoOpen] = useState(false);
  const [summaryError, setSummaryError] = useState<string | null>(null);

  useEffect(() => {
    void refreshGit(project.id);
    const onFocus = () => void refreshGit(project.id);
    window.addEventListener("focus", onFocus);
    const t = window.setInterval(() => void refreshGit(project.id), 30_000);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.clearInterval(t);
    };
  }, [project.id, refreshGit]);

  const restartClaude = async () => {
    if (!(await confirmDialog(t("project.confirmNewConversation")))) return;
    requestRestart(paneId(project.id, "claude"), true);
  };

  return (
    <header className="phead">
      <div className="phead__row">
        <span className="phead__dot" style={{ background: project.color }} />
        <h1 className="phead__name">{project.name}</h1>
        <StatusBadge status={status} />
        {git?.branch && (
          <span className="chip" title={t("project.gitBranch")}>
            {git.branch}
            {git.changed > 0 && <span className="chip__dirty">±{git.changed}</span>}
            {git.ahead > 0 && <span className="chip__meta">↑{git.ahead}</span>}
            {git.behind > 0 && <span className="chip__meta">↓{git.behind}</span>}
          </span>
        )}
        <button
          className="phead__path"
          title={t("project.openFolder")}
          onClick={() => void openPath(project.path)}
        >
          {project.path}
        </button>
        <div className="phead__actions">
          <button className="btn btn--ghost btn--small" onClick={() => void openPath(project.path)}>
            {t("menu.explorer")}
          </button>
          <button className="btn btn--ghost btn--small" onClick={() => setInfoOpen((v) => !v)}>
            {infoOpen ? t("project.hideInfo") : t("project.info")}
          </button>
          <button className="btn btn--ghost btn--small" onClick={restartClaude}>
            {t("project.newConversation")}
          </button>
          <button
            className="btn btn--ghost btn--small"
            onClick={() => set({ dialog: { kind: "edit", projectId: project.id } })}
          >
            {t("project.edit")}
          </button>
        </div>
      </div>
      {(project.description || title) && (
        <p className="phead__desc">
          {project.description}
          {title && <span className="phead__title">{project.description ? " · " : ""}{title}</span>}
        </p>
      )}
      {infoOpen && (
        <div className="phead__info">
          <section>
            <h2>
              {t("project.summary")}
              <button
                className="btn btn--ghost btn--small"
                disabled={summarizing}
                onClick={() => {
                  setSummaryError(null);
                  summarize(project.id).catch((e) => setSummaryError(String(e)));
                }}
              >
                {summarizing
                  ? t("project.summaryGenerating")
                  : project.summary
                    ? t("project.summaryUpdate")
                    : t("project.summaryGenerate")}
              </button>
            </h2>
            {summaryError && <p className="error">{summaryError}</p>}
            {project.summary ? (
              <p>
                {project.summary.text}
                <span className="muted">
                  {" "}
                  {t("project.summaryGenerated", { when: relativeTime(project.summary.generatedAt, t) })}
                </span>
              </p>
            ) : (
              <p className="muted">{t("project.summaryEmpty")}</p>
            )}
          </section>
          <section>
            <h2>{t("project.lastReply")}</h2>
            {message ? <p className="phead__msg">{message}</p> : <p className="muted">{t("project.noReply")}</p>}
          </section>
        </div>
      )}
    </header>
  );
}
