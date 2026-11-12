import { useEffect, useLayoutEffect, useRef, useState } from "react";

type Tip = { text: string; rect: DOMRect };

const DELAY_MS = 300;

/**
 * One tooltip for the whole window: any element with `data-tip` shows it on
 * hover or keyboard focus. Rendered at the top level so scroll containers
 * and overflow never clip it.
 */
export function Tooltip() {
  const [tip, setTip] = useState<Tip | null>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let timer = 0;
    let current: Element | null = null;
    const show = (el: Element) => {
      window.clearTimeout(timer);
      current = el;
      timer = window.setTimeout(() => {
        const text = el.getAttribute("data-tip");
        if (text && el.isConnected) setTip({ text, rect: el.getBoundingClientRect() });
      }, DELAY_MS);
    };
    const hide = () => {
      window.clearTimeout(timer);
      current = null;
      setTip(null);
    };
    const onOver = (e: Event) => {
      const el = (e.target as Element | null)?.closest?.("[data-tip]");
      if (el && el !== current) show(el);
      else if (!el && current) hide();
    };
    const onFocus = (e: FocusEvent) => {
      const el = (e.target as Element | null)?.closest?.("[data-tip]");
      if (el && (e.target as HTMLElement).matches(":focus-visible")) show(el);
    };
    document.addEventListener("mouseover", onOver);
    document.addEventListener("focusin", onFocus);
    document.addEventListener("focusout", hide);
    document.addEventListener("mousedown", hide);
    window.addEventListener("blur", hide);
    window.addEventListener("scroll", hide, true);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("mouseover", onOver);
      document.removeEventListener("focusin", onFocus);
      document.removeEventListener("focusout", hide);
      document.removeEventListener("mousedown", hide);
      window.removeEventListener("blur", hide);
      window.removeEventListener("scroll", hide, true);
    };
  }, []);

  // Below the element, centered, kept inside the window; above it if there
  // is no room below.
  useLayoutEffect(() => {
    if (!tip || !ref.current) return setPos(null);
    const box = ref.current.getBoundingClientRect();
    const gap = 6;
    let x = tip.rect.left + tip.rect.width / 2 - box.width / 2;
    x = Math.max(4, Math.min(x, window.innerWidth - box.width - 4));
    let y = tip.rect.bottom + gap;
    if (y + box.height > window.innerHeight - 4) y = tip.rect.top - gap - box.height;
    setPos({ x, y });
  }, [tip]);

  if (!tip) return null;
  return (
    <div
      ref={ref}
      className="tooltip"
      role="tooltip"
      style={{ left: pos?.x ?? 0, top: pos?.y ?? 0, visibility: pos ? "visible" : "hidden" }}
    >
      {tip.text}
    </div>
  );
}
