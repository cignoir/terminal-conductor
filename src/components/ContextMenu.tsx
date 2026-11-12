import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useStore } from "../store";

export type MenuItem = {
  label: string;
  onSelect: () => void;
  danger?: boolean;
};

export type MenuState = { x: number; y: number; items: MenuItem[] } | null;

/** Opens the app's own context menu at the pointer. */
export function openContextMenu(e: React.MouseEvent, items: MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  useStore.getState().set({ contextMenu: { x: e.clientX, y: e.clientY, items } });
}

export function ContextMenu() {
  const menu = useStore((s) => s.contextMenu);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const close = () => useStore.getState().set({ contextMenu: null });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return setPos(null);
    const r = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(menu.x, window.innerWidth - r.width - 4),
      y: Math.min(menu.y, window.innerHeight - r.height - 4),
    });
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
    };
  }, [menu]);

  if (!menu) return null;
  return (
    <div
      className="menu-overlay"
      onMouseDown={close}
      onContextMenu={(e) => {
        e.preventDefault();
        close();
      }}
    >
      <div
        ref={ref}
        className="menu"
        role="menu"
        style={{ left: pos?.x ?? menu.x, top: pos?.y ?? menu.y, visibility: pos ? "visible" : "hidden" }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {menu.items.map((item, i) => (
          <button
            key={i}
            role="menuitem"
            className={`menu__item ${item.danger ? "menu__item--danger" : ""}`}
            onClick={() => {
              close();
              item.onSelect();
            }}
          >
            {item.label}
          </button>
        ))}
      </div>
    </div>
  );
}
