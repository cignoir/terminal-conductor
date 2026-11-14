import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "../api";
import { PROJECT_COLORS, folderName, useStore } from "../store";
import type { Project, ShellName } from "../types";
import { applySettingsToAll } from "../terminal/cache";
import { confirmDialog } from "../confirm";
import { UpdatePanel } from "./UpdatePanel";
import { LANGUAGES, useT, type LanguageSetting, type MessageKey } from "../i18n";

function Modal({
  title,
  children,
  onClose,
  footer,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  footer: ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <h2 className="modal__title">{title}</h2>
        <div className="modal__body">{children}</div>
        <div className="modal__footer">{footer}</div>
      </div>
    </div>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="field">
      <span className="field__label">{label}</span>
      {children}
      {hint && <span className="field__hint">{hint}</span>}
    </label>
  );
}

const SHELLS: { value: ShellName | ""; label: MessageKey }[] = [
  { value: "", label: "shell.default" },
  { value: "cmd", label: "shell.cmd" },
  { value: "pwsh", label: "shell.pwsh" },
  { value: "powershell", label: "shell.powershell" },
];

export function ProjectDialog({ projectId }: { projectId?: string }) {
  const t = useT();
  const existing = useStore((s) => s.projects.find((p) => p.id === projectId));
  const set = useStore((s) => s.set);
  const addProjects = useStore((s) => s.addProjects);
  const updateProject = useStore((s) => s.updateProject);
  const removeProject = useStore((s) => s.removeProject);
  const openProject = useStore((s) => s.openProject);
  const autoStartClaude = useStore((s) => s.settings.autoStartClaude);
  const [draft, setDraft] = useState<Partial<Project>>(
    existing ?? {
      name: "",
      path: "",
      description: "",
      tags: [],
      autoStart: false,
      continueOnFirstOpen: true,
    },
  );
  const [tags, setTags] = useState((existing?.tags ?? []).join(", "));
  const [error, setError] = useState<string | null>(null);
  const close = () => set({ dialog: null });
  const patch = (p: Partial<Project>) => setDraft((d) => ({ ...d, ...p }));

  const pickFolder = async () => {
    const dir = await open({ directory: true, defaultPath: draft.path || undefined });
    if (typeof dir === "string") {
      patch({ path: dir, name: draft.name || folderName(dir) });
    }
  };

  const save = async () => {
    // Pasted paths often carry spaces (incl. full-width) or the quotes that
    // Explorer's "Copy as path" adds.
    const path = (draft.path ?? "").trim().replace(/^"(.*)"$/, "$1").trim();
    if (!path || !(await api.pathExists(path))) {
      setError(t("dialog.folderNotFound"));
      return;
    }
    const value: Partial<Project> = {
      ...draft,
      path,
      name: (draft.name || folderName(path)).trim(),
      tags: tags.split(/[,、]/).map((t) => t.trim()).filter(Boolean),
    };
    if (existing) {
      updateProject(existing.id, value);
    } else {
      const ids = addProjects([value]);
      if (!ids.length) {
        setError(t("dialog.alreadyAdded"));
        return;
      }
      openProject(ids[0]);
    }
    close();
  };

  return (
    <Modal
      title={existing ? t("dialog.editTitle") : t("dialog.addTitle")}
      onClose={close}
      footer={
        <>
          {existing && (
            <button
              className="btn btn--danger btn--small"
              onClick={async () => {
                if (await confirmDialog(t("dialog.confirmRemove", { name: existing.name }))) {
                  removeProject(existing.id);
                  close();
                }
              }}
            >
              {t("dialog.remove")}
            </button>
          )}
          <span className="spacer" />
          <button className="btn btn--ghost" onClick={close}>
            {t("dialog.cancel")}
          </button>
          <button className="btn btn--primary" onClick={() => void save()}>
            {existing ? t("dialog.save") : t("dialog.addAndOpen")}
          </button>
        </>
      }
    >
      <Field label={t("dialog.folder")}>
        <div className="field__row">
          <input
            value={draft.path ?? ""}
            onChange={(e) => patch({ path: e.target.value })}
            placeholder="C:\path\to\project"
            spellCheck={false}
          />
          <button className="btn btn--small" onClick={() => void pickFolder()}>
            {t("dialog.browse")}
          </button>
        </div>
      </Field>
      <Field label={t("dialog.name")}>
        <input value={draft.name ?? ""} onChange={(e) => patch({ name: e.target.value })} />
      </Field>
      <Field label={t("dialog.description")} hint={t("dialog.descriptionHint")}>
        <input
          value={draft.description ?? ""}
          onChange={(e) => patch({ description: e.target.value })}
          placeholder={t("dialog.descriptionPlaceholder")}
        />
      </Field>
      <Field label={t("dialog.tags")} hint={t("dialog.tagsHint")}>
        <input value={tags} onChange={(e) => setTags(e.target.value)} placeholder={t("dialog.tagsPlaceholder")} />
      </Field>
      <Field label={t("dialog.color")}>
        <div className="swatches">
          {PROJECT_COLORS.map((c) => (
            <button
              key={c}
              className={`swatch ${draft.color === c ? "swatch--on" : ""}`}
              style={{ background: c }}
              onClick={() => patch({ color: c })}
              aria-label={c}
            />
          ))}
        </div>
      </Field>
      <Field label={t("dialog.shell")}>
        <select
          value={draft.shell ?? ""}
          onChange={(e) => patch({ shell: (e.target.value || undefined) as ShellName | undefined })}
        >
          {SHELLS.map((s) => (
            <option key={s.value} value={s.value}>
              {t(s.label)}
            </option>
          ))}
        </select>
      </Field>
      <label className="check">
        <input
          type="checkbox"
          checked={!!draft.autoStart}
          onChange={(e) => patch({ autoStart: e.target.checked })}
        />
        {t("dialog.autoStart")}
      </label>
      {!existing && autoStartClaude && (
        <div className="check-block">
          <label className="check">
            <input
              type="checkbox"
              checked={!!draft.continueOnFirstOpen}
              onChange={(e) => patch({ continueOnFirstOpen: e.target.checked })}
            />
            {t("dialog.continue")}
          </label>
          <span className="field__hint">
            {t("dialog.continueHint")}
          </span>
        </div>
      )}
      {error && <p className="error">{error}</p>}
    </Modal>
  );
}

export function ImportDialog() {
  const t = useT();
  const set = useStore((s) => s.set);
  const projects = useStore((s) => s.projects);
  const addProjects = useStore((s) => s.addProjects);
  const [root, setRoot] = useState("");
  const [found, setFound] = useState<string[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const known = new Set(projects.map((p) => p.path.toLowerCase()));
  const close = () => set({ dialog: null });

  const scan = async (dir: string) => {
    setRoot(dir);
    setBusy(true);
    try {
      const list = await api.scanProjects(dir);
      setFound(list);
      setPicked(new Set(list.filter((p) => !known.has(p.toLowerCase()))));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      title={t("import.title")}
      onClose={close}
      footer={
        <>
          <span className="spacer" />
          <button className="btn btn--ghost" onClick={close}>
            {t("dialog.cancel")}
          </button>
          <button
            className="btn btn--primary"
            disabled={!picked.size}
            onClick={() => {
              addProjects([...picked].map((path) => ({ path })));
              close();
            }}
          >
            {t("import.add", { count: picked.size })}
          </button>
        </>
      }
    >
      <p className="muted">
        {t("import.help")}
      </p>
      <div className="field__row">
        <input value={root} readOnly placeholder={t("import.parent")} />
        <button
          className="btn btn--small"
          disabled={busy}
          onClick={async () => {
            const dir = await open({ directory: true });
            if (typeof dir === "string") await scan(dir);
          }}
        >
          {t("dialog.browse")}
        </button>
      </div>
      {busy && <p className="muted">{t("import.searching")}</p>}
      {found && (
        <ul className="import-list">
          {found.length === 0 && <li className="muted">{t("import.none")}</li>}
          {found.map((p) => {
            const already = known.has(p.toLowerCase());
            return (
              <li key={p}>
                <label className="check">
                  <input
                    type="checkbox"
                    disabled={already}
                    checked={picked.has(p)}
                    onChange={(e) => {
                      const next = new Set(picked);
                      if (e.target.checked) next.add(p);
                      else next.delete(p);
                      setPicked(next);
                    }}
                  />
                  <span className="import-list__name">{folderName(p)}</span>
                  <span className="muted import-list__path">{already ? t("import.already") : p}</span>
                </label>
              </li>
            );
          })}
        </ul>
      )}
    </Modal>
  );
}

function HistorySettings({
  enabled,
  onChange,
}: {
  enabled: boolean;
  onChange: (v: boolean) => void;
}) {
  const t = useT();
  const [note, setNote] = useState<string | null>(null);
  return (
    <div className="check-block">
      <label className="check">
        <input type="checkbox" checked={enabled} onChange={(e) => onChange(e.target.checked)} />
        {t("settings.historySearch")}
      </label>
      <span className="field__hint">{t("settings.historySearchHint")}</span>
      <div>
        <button
          className="btn btn--small btn--danger"
          onClick={async () => {
            if (!(await confirmDialog(t("settings.historyDeleteConfirm")))) return;
            try {
              await api.historyDelete();
              setNote(t("settings.historyDeleted"));
            } catch (e) {
              setNote(String(e));
            }
          }}
        >
          {t("settings.historyDelete")}
        </button>
        {note && <span className="field__hint"> {note}</span>}
      </div>
    </div>
  );
}

export function SettingsDialog() {
  const t = useT();
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const claudePath = useStore((s) => s.claudePath);
  const dataDir = useStore((s) => s.dataDir);
  const set = useStore((s) => s.set);
  const [draft, setDraft] = useState(settings);
  const close = () => set({ dialog: null });

  return (
    <Modal
      title={t("settings.title")}
      onClose={close}
      footer={
        <>
          <span className="spacer" />
          <button className="btn btn--ghost" onClick={close}>
            {t("dialog.cancel")}
          </button>
          <button
            className="btn btn--primary"
            onClick={() => {
              setSettings(draft);
              applySettingsToAll(draft);
              close();
            }}
          >
            {t("dialog.save")}
          </button>
        </>
      }
    >
      <Field label={t("settings.language")}>
        <select
          value={draft.language}
          onChange={(e) => setDraft({ ...draft, language: e.target.value as LanguageSetting })}
        >
          <option value="auto">{t("settings.languageAuto")}</option>
          {LANGUAGES.map((l) => (
            <option key={l.value} value={l.value}>
              {l.label}
            </option>
          ))}
        </select>
      </Field>
      <Field label={t("settings.font")} hint={t("settings.fontHint")}>
        <input
          value={draft.fontFamily}
          onChange={(e) => setDraft({ ...draft, fontFamily: e.target.value })}
          spellCheck={false}
        />
      </Field>
      <div className="field__grid">
        <Field label={t("settings.fontSize")}>
          <input
            type="number"
            min={8}
            max={32}
            value={draft.fontSize}
            onChange={(e) => setDraft({ ...draft, fontSize: Number(e.target.value) || 14 })}
          />
        </Field>
        <Field label={t("settings.scrollback")}>
          <input
            type="number"
            min={1000}
            max={100000}
            step={1000}
            value={draft.scrollback}
            onChange={(e) => setDraft({ ...draft, scrollback: Number(e.target.value) || 5000 })}
          />
        </Field>
      </div>
      <Field label={t("settings.defaultShell")}>
        <select
          value={draft.defaultShell}
          onChange={(e) => setDraft({ ...draft, defaultShell: e.target.value as ShellName })}
        >
          {SHELLS.filter((s) => s.value).map((s) => (
            <option key={s.value} value={s.value}>
              {t(s.label)}
            </option>
          ))}
        </select>
      </Field>
      <label className="check">
        <input
          type="checkbox"
          checked={draft.notifications}
          onChange={(e) => setDraft({ ...draft, notifications: e.target.checked })}
        />
        {t("settings.notifications")}
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={draft.autoOpenImages}
          onChange={(e) => setDraft({ ...draft, autoOpenImages: e.target.checked })}
        />
        {t("settings.autoOpenImages")}
      </label>
      <div className="check-block">
        <label className="check">
          <input
            type="checkbox"
            checked={draft.autoStartClaude}
            onChange={(e) => setDraft({ ...draft, autoStartClaude: e.target.checked })}
          />
          {t("settings.autoStartClaude")}
        </label>
        <span className="field__hint">{t("settings.autoStartClaudeHint")}</span>
      </div>
      <HistorySettings
        enabled={draft.historySearch}
        onChange={(v) => setDraft({ ...draft, historySearch: v })}
      />
      <UpdatePanel />
      <dl className="about">
        <dt>Claude Code</dt>
        <dd>{claudePath ?? t("settings.claudeMissing")}</dd>
        <dt>{t("settings.dataDir")}</dt>
        <dd>{dataDir}</dd>
      </dl>
    </Modal>
  );
}
