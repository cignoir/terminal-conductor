import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { ChevronLeft, ChevronRight, Copy, ExternalLink, FolderOpen, X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { imageUrl } from "../api";
import { useT } from "../i18n";
import { useStore } from "../store";
import type { ImageItem } from "../types";
import { copyImage, imageMenu } from "./ImagePanel";
import { IconButton } from "./IconButton";

const EMPTY: ImageItem[] = [];
const MIN_SCALE = 0.05;
const MAX_SCALE = 32;

type View = { scale: number; x: number; y: number };

/** Full-window viewer for an image from the image panel. */
export function Lightbox() {
  const box = useStore((s) => s.lightbox);
  const items = useStore((s) => (box ? (s.images[box.paneId] ?? EMPTY) : EMPTY));
  if (!box) return null;
  const index = items.findIndex((i) => i.id === box.id);
  if (index < 0) return null;
  return <Viewer key={items[index].id} paneId={box.paneId} items={items} index={index} />;
}

function Viewer({ paneId, items, index }: { paneId: string; items: ImageItem[]; index: number }) {
  const t = useT();
  const item = items[index];
  const video = item.kind === "video";
  const rootRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [broken, setBroken] = useState(false);
  const [view, setView] = useState<View | null>(null);
  const dragRef = useRef<{ x: number; y: number; vx: number; vy: number } | null>(null);
  // A click closes the viewer only if the press also began on the backdrop
  // (releasing a drag of the image must not close it).
  const pressedBackdrop = useRef(false);

  const close = () => useStore.getState().set({ lightbox: null });
  const go = (d: number) => {
    const next = items[index + d];
    if (next) useStore.getState().set({ lightbox: { paneId, id: next.id } });
  };

  // Take focus so keys go to the viewer, not to the terminal behind it
  // (Esc there would interrupt Claude); give it back on close.
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    rootRef.current?.focus();
    return () => prev?.focus();
  }, []);

  const fit = (): View | null => {
    const stage = stageRef.current;
    if (!stage || !natural) return null;
    const { clientWidth: sw, clientHeight: sh } = stage;
    // Raster images are not enlarged past 100%; vector ones fill the stage.
    const cap = /\.svg$/i.test(item.name) ? Infinity : 1;
    const scale = Math.min(cap, sw / natural.w, sh / natural.h);
    return { scale, x: (sw - natural.w * scale) / 2, y: (sh - natural.h * scale) / 2 };
  };

  useLayoutEffect(() => {
    if (natural && !video) setView(fit());
    // fit() reads the stage size; it only needs to run when the image loads.
  }, [natural]);

  const zoomAt = (cx: number, cy: number, scale: number) =>
    setView((v) => {
      if (!v) return v;
      const s = Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale));
      return { scale: s, x: cx - ((cx - v.x) * s) / v.scale, y: cy - ((cy - v.y) * s) / v.scale };
    });

  const stagePoint = (e: { clientX: number; clientY: number }) => {
    const r = stageRef.current!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top] as const;
  };

  const actualSize = (cx?: number, cy?: number) => {
    const stage = stageRef.current;
    if (!stage || !view) return;
    zoomAt(cx ?? stage.clientWidth / 2, cy ?? stage.clientHeight / 2, 1);
  };

  const isFit = (() => {
    const f = view && fit();
    return !!f && Math.abs(f.scale - view.scale) < 1e-3;
  })();

  const zoomBy = (f: number) => {
    const stage = stageRef.current;
    if (!stage || !view) return;
    zoomAt(stage.clientWidth / 2, stage.clientHeight / 2, view.scale * f);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape") close();
    else if (e.key === "ArrowLeft") go(-1);
    else if (e.key === "ArrowRight") go(1);
    else if (e.key === "0") setView(fit());
    else if (e.key === "1") actualSize();
    else if (e.key === "+" || e.key === "=") zoomBy(1.25);
    else if (e.key === "-") zoomBy(1 / 1.25);
    else return;
    e.preventDefault();
  };

  return (
    <div
      ref={rootRef}
      className="lightbox"
      role="dialog"
      aria-modal="true"
      aria-label={item.name}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div
        ref={stageRef}
        className="lightbox__stage"
        onClick={(e) => {
          if (pressedBackdrop.current && e.target === e.currentTarget) close();
        }}
        onWheel={(e) => {
          if (!view) return;
          const [cx, cy] = stagePoint(e);
          zoomAt(cx, cy, view.scale * (e.deltaY < 0 ? 1.15 : 1 / 1.15));
        }}
        onPointerDown={(e) => {
          pressedBackdrop.current = e.target === e.currentTarget;
          if (e.button !== 0 || !view || pressedBackdrop.current) return;
          (e.target as Element).setPointerCapture(e.pointerId);
          dragRef.current = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y };
        }}
        onPointerMove={(e) => {
          const d = dragRef.current;
          if (!d) return;
          setView((v) => v && { ...v, x: d.vx + e.clientX - d.x, y: d.vy + e.clientY - d.y });
        }}
        onPointerUp={() => {
          dragRef.current = null;
        }}
        onDoubleClick={(e) => {
          if (video || e.target === e.currentTarget) return;
          const [cx, cy] = stagePoint(e);
          if (isFit) actualSize(cx, cy);
          else setView(fit());
        }}
      >
        {broken ? (
          <p className="lightbox__missing">{t("images.missing")}</p>
        ) : video ? (
          <video
            className="lightbox__video"
            src={imageUrl(item, "full")}
            controls
            autoPlay
            onLoadedMetadata={(e) =>
              setNatural({ w: e.currentTarget.videoWidth, h: e.currentTarget.videoHeight })
            }
            onError={() => setBroken(true)}
            onContextMenu={(e) => imageMenu(e, paneId, item, t)}
          />
        ) : (
          <img
            src={imageUrl(item, "full")}
            alt={item.name}
            draggable={false}
            onLoad={(e) => setNatural({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })}
            onError={() => setBroken(true)}
            onContextMenu={(e) => imageMenu(e, paneId, item, t)}
            style={
              view
                ? {
                    width: natural!.w,
                    height: natural!.h,
                    transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})`,
                  }
                : { visibility: "hidden" }
            }
          />
        )}
        {index > 0 && (
          <button
            type="button"
            className="lightbox__nav lightbox__nav--prev"
            aria-label={t("images.prev")}
            onClick={() => go(-1)}
          >
            <ChevronLeft size={28} aria-hidden />
          </button>
        )}
        {index < items.length - 1 && (
          <button
            type="button"
            className="lightbox__nav lightbox__nav--next"
            aria-label={t("images.next")}
            onClick={() => go(1)}
          >
            <ChevronRight size={28} aria-hidden />
          </button>
        )}
      </div>
      <footer className="lightbox__bar">
        <div className="lightbox__info">
          <span className="lightbox__name">{item.name}</span>
          <span className="lightbox__path">{item.path}</span>
        </div>
        <span className="lightbox__meta">
          {natural && `${natural.w} × ${natural.h}`}
          {view && ` · ${Math.round(view.scale * 100)}%`}
          {items.length > 1 && ` · ${index + 1} / ${items.length}`}
        </span>
        {!video && (
          <>
            <button type="button" className="btn btn--small" onClick={() => (isFit ? actualSize() : setView(fit()))}>
              {isFit ? t("images.actual") : t("images.fit")}
            </button>
            <IconButton icon={Copy} label={t("images.copyImage")} onClick={() => void copyImage(item, t)} />
          </>
        )}
        <IconButton icon={FolderOpen} label={t("images.reveal")} onClick={() => void revealItemInDir(item.path)} />
        <IconButton icon={ExternalLink} label={t("images.open")} onClick={() => void openPath(item.path)} />
        <IconButton icon={X} label={t("images.closeViewer")} shortcut="Esc" onClick={close} />
      </footer>
    </div>
  );
}
