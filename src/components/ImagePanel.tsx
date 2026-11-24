import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Globe, ImageOff, Music, PanelRightClose, Play, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, imageUrl } from "../api";
import { useT } from "../i18n";
import { flushDocuments, useStore } from "../store";
import { type ImageItem, type Project, type SideTab, paneId } from "../types";
import { relativeTime } from "../util";
import { confirmDialog } from "../confirm";
import { openContextMenu } from "./ContextMenu";
import { highlightInTerminal } from "../terminal/cache";
import { IconButton } from "./IconButton";

export const PANEL_MIN = 200;
export const PANEL_MAX = 720;
const EMPTY: ImageItem[] = [];

/** Copies an image and reports the outcome at the bottom of the window. */
export async function copyImage(item: ImageItem, t: ReturnType<typeof useT>) {
  const set = useStore.getState().set;
  try {
    await api.imageCopy(item.id);
    set({ notice: t("images.copied") });
  } catch (e) {
    set({ notice: t("images.copyFailed", { error: String(e) }) });
  }
}

export function imageMenu(
  e: React.MouseEvent,
  pid: string,
  item: ImageItem,
  t: ReturnType<typeof useT>,
) {
  openContextMenu(e, [
    {
      label: item.kind === "html" ? t("images.openBrowser") : t("images.open"),
      onSelect: () => void openPath(item.path),
    },
    { label: t("images.reveal"), onSelect: () => void revealItemInDir(item.path) },
    { label: t("images.copyPath"), onSelect: () => void writeText(item.path) },
    ...(item.kind === "image"
      ? [{ label: t("images.copyImage"), onSelect: () => void copyImage(item, t) }]
      : []),
    {
      label: t("images.remove"),
      danger: true,
      onSelect: () =>
        void api.imagesRemove(pid, item.id).then((items) => useStore.getState().setImages(pid, items)),
    },
  ]);
}

/** The right-hand panel: media Claude mentioned in the top pane, and notes. */
export function ImagePanel({ project }: { project: Project }) {
  const t = useT();
  const pid = paneId(project.id, "claude");
  const items = useStore((s) => s.images[pid] ?? EMPTY);
  const hasNote = useStore((s) => !!s.notes[project.id]);
  const width = useStore((s) => s.settings.imagePanelWidth);
  const setSettings = useStore((s) => s.setSettings);
  const toggleImages = useStore((s) => s.toggleImages);
  const updateProject = useStore((s) => s.updateProject);
  const tab = project.sideTab ?? "media";
  const [drag, setDrag] = useState<number | null>(null);
  const dragging = drag !== null;

  useEffect(() => {
    // The first call may start reading the saved session, whose result
    // arrives as an event, possibly before this reply; don't overwrite it.
    void api.imagesList(pid).then((list) => {
      if (!useStore.getState().images[pid]) useStore.getState().setImages(pid, list);
    });
  }, [pid]);

  // Apply the width only when the drag ends: every width change resizes the
  // PTY, which makes the program in it redraw.
  useEffect(() => {
    if (!dragging) return;
    let last: number | null = null;
    const move = (e: PointerEvent) => {
      last = Math.min(PANEL_MAX, Math.max(PANEL_MIN, window.innerWidth - e.clientX));
      setDrag(last);
    };
    const up = () => {
      if (last !== null) setSettings({ imagePanelWidth: Math.round(last) });
      setDrag(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [dragging, setSettings]);

  const clear = async () => {
    if (!(await confirmDialog(t("images.confirmClear")))) return;
    await api.imagesClear(pid);
    useStore.getState().setImages(pid, []);
  };

  const tabButton = (id: SideTab, label: string, extra?: React.ReactNode) => (
    <button
      type="button"
      role="tab"
      aria-selected={tab === id}
      className={`imgpanel__tab ${tab === id ? "imgpanel__tab--on" : ""}`}
      onClick={() => updateProject(project.id, { sideTab: id })}
    >
      {label}
      {extra}
    </button>
  );

  return (
    <aside className="imgpanel" style={{ width }} aria-label={t(tab === "media" ? "images.title" : "notes.title")}>
      <div
        className="imgpanel__handle"
        title={t("images.resize")}
        onPointerDown={(e) => {
          e.preventDefault();
          setDrag(width);
        }}
      />
      {drag !== null && <div className="imgpanel__ghost" style={{ left: window.innerWidth - drag }} />}
      <header className="imgpanel__head">
        <div className="imgpanel__tabs" role="tablist">
          {tabButton(
            "media",
            t("images.title"),
            items.length > 0 && <span className="muted"> {items.length}</span>,
          )}
          {tabButton("notes", t("notes.title"), hasNote && <span className="imgpanel__dot" aria-hidden />)}
        </div>
        {tab === "media" && (
          <IconButton
            icon={Trash2}
            size={14}
            label={t("images.clear")}
            disabled={!items.length}
            onClick={() => void clear()}
          />
        )}
        <IconButton
          icon={PanelRightClose}
          size={14}
          label={t("images.close")}
          onClick={() => toggleImages(project.id, false)}
        />
      </header>
      {tab === "notes" ? (
        <NotesEditor projectId={project.id} />
      ) : items.length === 0 ? (
        <p className="imgpanel__empty muted">{t("images.empty")}</p>
      ) : (
        <ul className="imgpanel__grid">
          {items.map((item) => (
            <Thumb key={item.id} pid={pid} item={item} />
          ))}
        </ul>
      )}
    </aside>
  );
}

/** A plain text area for the project's notes, saved as you type. */
function NotesEditor({ projectId }: { projectId: string }) {
  const t = useT();
  const text = useStore((s) => s.notes[projectId] ?? "");
  const setNote = useStore((s) => s.setNote);
  return (
    <div className="notes">
      <textarea
        className="notes__text"
        value={text}
        onChange={(e) => setNote(projectId, e.target.value)}
        // Save right away when leaving, in case the app quits soon after.
        onBlur={() => void flushDocuments()}
        placeholder={t("notes.placeholder")}
        spellCheck={false}
        aria-label={t("notes.title")}
      />
      <p className="notes__hint muted">{t("notes.hint")}</p>
    </div>
  );
}

function Thumb({ pid, item }: { pid: string; item: ImageItem }) {
  const t = useT();
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [item.modified]);
  // Pointing at a thumbnail marks where Claude wrote its file name.
  const unmark = useRef<(() => void) | null>(null);
  const mark = () => {
    unmark.current?.();
    unmark.current = highlightInTerminal(pid, item.name);
  };
  const clearMark = () => {
    unmark.current?.();
    unmark.current = null;
  };
  useEffect(() => clearMark, []);
  return (
    <li>
      <button
        type="button"
        className="imgthumb"
        data-tip={item.path}
        onClick={() => {
          clearMark();
          // Pages open in the browser; they are never shown inside the app.
          if (item.kind === "html") void openPath(item.path);
          else useStore.getState().set({ lightbox: { paneId: pid, id: item.id } });
        }}
        onMouseEnter={mark}
        onMouseLeave={clearMark}
        onFocus={mark}
        onBlur={clearMark}
        onContextMenu={(e) => imageMenu(e, pid, item, t)}
      >
        <span className="imgthumb__img">
          {broken ? (
            <span className="imgthumb__missing">
              <ImageOff size={20} aria-hidden />
              {t("images.missing")}
            </span>
          ) : item.kind === "audio" || item.kind === "html" ? (
            <span className={`imgthumb__icon imgthumb__icon--${item.kind}`}>
              {item.kind === "audio" ? <Music size={28} aria-hidden /> : <Globe size={28} aria-hidden />}
              <span>{item.name.split(".").pop()?.toUpperCase()}</span>
            </span>
          ) : item.kind === "video" ? (
            <>
              {/* The first frame stands in for a thumbnail. */}
              <video
                src={`${imageUrl(item, "full")}#t=0.1`}
                preload="metadata"
                muted
                playsInline
                tabIndex={-1}
                onError={() => setBroken(true)}
              />
              <span className="imgthumb__play" aria-hidden>
                <Play size={16} fill="currentColor" />
              </span>
            </>
          ) : (
            <img
              src={imageUrl(item, "thumb")}
              alt=""
              loading="lazy"
              decoding="async"
              draggable={false}
              onError={() => setBroken(true)}
            />
          )}
        </span>
        <span className="imgthumb__name">{item.name}</span>
        <span className="imgthumb__meta">{relativeTime(item.foundAt, t)}</span>
      </button>
    </li>
  );
}
