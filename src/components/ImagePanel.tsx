import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { ImageOff, PanelRightClose, Play, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, imageUrl } from "../api";
import { useT } from "../i18n";
import { useStore } from "../store";
import { type ImageItem, type Project, paneId } from "../types";
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
    { label: t("images.open"), onSelect: () => void openPath(item.path) },
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

/** The right-hand panel listing images Claude mentioned in the top pane. */
export function ImagePanel({ project }: { project: Project }) {
  const t = useT();
  const pid = paneId(project.id, "claude");
  const items = useStore((s) => s.images[pid] ?? EMPTY);
  const width = useStore((s) => s.settings.imagePanelWidth);
  const setSettings = useStore((s) => s.setSettings);
  const toggleImages = useStore((s) => s.toggleImages);
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

  return (
    <aside className="imgpanel" style={{ width }} aria-label={t("images.title")}>
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
        <span className="imgpanel__title">
          {t("images.title")}
          {items.length > 0 && <span className="muted"> {items.length}</span>}
        </span>
        <IconButton
          icon={Trash2}
          size={14}
          label={t("images.clear")}
          disabled={!items.length}
          onClick={() => void clear()}
        />
        <IconButton
          icon={PanelRightClose}
          size={14}
          label={t("images.close")}
          shortcut="Ctrl+Shift+I"
          onClick={() => toggleImages(project.id, false)}
        />
      </header>
      {items.length === 0 ? (
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
          useStore.getState().set({ lightbox: { paneId: pid, id: item.id } });
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
