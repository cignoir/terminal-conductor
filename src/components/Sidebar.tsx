import { useMemo, useState } from "react";
import { lastActivity, lastMessage, projectStatus, useStore } from "../store";
import type { Project } from "../types";
import { firstLine, matchScore, relativeTime } from "../util";
import { StatusDot } from "./StatusBadge";
import { confirmDialog } from "../confirm";
import { useT } from "../i18n";
import { openPath } from "@tauri-apps/plugin-opener";
import { openContextMenu } from "./ContextMenu";
import { updateNeedsAttention, useUpdater } from "../updater";
import { ChevronDown, ChevronUp, FolderInput, Plus, Settings, TextSearch, X } from "lucide-react";
import { IconButton } from "./IconButton";

export function Sidebar() {
  const t = useT();
  const projects = useStore((s) => s.projects);
  const workspace = useStore((s) => s.workspace);
  const set = useStore((s) => s.set);
  const [filter, setFilter] = useState("");
  const updateBadge = useUpdater((s) => updateNeedsAttention(s.phase));

  const byId = useMemo(
    () => new Map(projects.map((p) => [p.id, p])),
    [projects],
  );
  const openSet = new Set(workspace.openProjectIds);
  const match = (p: Project) =>
    !filter ||
    matchScore(filter, [p.name, p.description, p.tags.join(" "), p.path, p.summary?.text ?? ""]) >= 0;

  const open = workspace.openProjectIds
    .map((id) => byId.get(id))
    .filter((p): p is Project => !!p && match(p));
  const closed = projects
    .filter((p) => !openSet.has(p.id) && match(p))
    .sort((a, b) => a.name.localeCompare(b.name, "ja"));

  return (
    <aside className="sidebar">
      <div className="sidebar__top">
        <span className="brand">Terminal Conductor</span>
        <IconButton
          icon={Settings}
          label={updateBadge ? t("sidebar.settingsUpdate") : t("sidebar.settings")}
          shortcut="Ctrl+,"
          className="icon-btn--badge"
          data-badge={updateBadge ? "" : undefined}
          onClick={() => set({ dialog: { kind: "settings" } })}
        />
      </div>
      <div className="sidebar__search">
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder={t("sidebar.filter")}
          spellCheck={false}
        />
      </div>
      <div className="sidebar__list">
        {open.length > 0 && <h3 className="sidebar__heading">{t("sidebar.open")}</h3>}
        {open.map((p, i) => (
          <ProjectRow key={p.id} project={p} open index={i} />
        ))}
        {closed.length > 0 && <h3 className="sidebar__heading">{t("sidebar.others")}</h3>}
        {closed.map((p) => (
          <ProjectRow key={p.id} project={p} open={false} />
        ))}
        {projects.length === 0 && (
          <p className="sidebar__empty">
            {t("sidebar.empty")}
          </p>
        )}
      </div>
      <div className="sidebar__bottom">
        <IconButton icon={Plus} label={t("dialog.addTitle")} onClick={() => set({ dialog: { kind: "add" } })} />
        <IconButton
          icon={FolderInput}
          label={t("sidebar.import")}
          onClick={() => set({ dialog: { kind: "import" } })}
        />
        <IconButton
          icon={TextSearch}
          label={t("history.title")}
          shortcut="Ctrl+Shift+F"
          onClick={() => set({ dialog: { kind: "history" } })}
        />
      </div>
    </aside>
  );
}

function ProjectRow({
  project,
  open,
  index,
}: {
  project: Project;
  open: boolean;
  index?: number;
}) {
  const t = useT();
  const active = useStore((s) => s.workspace.activeProjectId === project.id);
  const status = useStore((s) => projectStatus(s, project.id));
  const unread = useStore((s) => !!s.unread[project.id]);
  const activity = useStore((s) => lastActivity(s, project.id));
  const message = useStore((s) => lastMessage(s, project.id));
  const activate = useStore((s) => s.activate);
  const closeProject = useStore((s) => s.closeProject);
  const moveOpen = useStore((s) => s.moveOpen);
  const set = useStore((s) => s.set);

  const confirmClose = async () => {
    if (await confirmDialog(t("sidebar.confirmClose", { name: project.name })))
      closeProject(project.id);
  };
  const onContextMenu = (e: React.MouseEvent) =>
    openContextMenu(e, [
      { label: t("project.openFolder"), onSelect: () => void openPath(project.path) },
      { label: t("menu.open"), onSelect: () => activate(project.id) },
      { label: t("project.edit"), onSelect: () => set({ dialog: { kind: "edit", projectId: project.id } }) },
      ...(open ? [{ label: t("menu.close"), onSelect: () => void confirmClose(), danger: true }] : []),
    ]);

  const sub = project.description || firstLine(message) || project.path;

  return (
    <div
      className={`row ${active ? "row--active" : ""} ${unread ? "row--unread" : ""}`}
      onClick={() => activate(project.id)}
      onContextMenu={onContextMenu}
      title={[project.name, project.description, project.summary?.text, project.path]
        .filter(Boolean)
        .join("\n")}
    >
      <span className="row__color" style={{ background: project.color }} />
      <div className="row__body">
        <div className="row__line">
          <StatusDot status={open ? status : undefined} unread={unread} />
          <span className="row__name">{project.name}</span>
          {index !== undefined && index < 9 && (
            <span className="row__key">^{index + 1}</span>
          )}
          <span className="row__time">{relativeTime(activity, t)}</span>
        </div>
        <div className="row__sub">{sub}</div>
      </div>
      {open && (
        <div className="row__tools" onClick={(e) => e.stopPropagation()}>
          <IconButton icon={ChevronUp} size={14} label={t("sidebar.moveUp")} onClick={() => moveOpen(project.id, -1)} />
          <IconButton icon={ChevronDown} size={14} label={t("sidebar.moveDown")} onClick={() => moveOpen(project.id, 1)} />
          <IconButton icon={X} size={14} label={t("sidebar.close")} onClick={() => void confirmClose()} />
        </div>
      )}
    </div>
  );
}
