import { useEffect, useMemo, useRef, useState } from "react";
import { lastActivity, projectStatus, useStore } from "../store";
import { matchScore, relativeTime } from "../util";
import { StatusDot } from "./StatusBadge";
import { useT } from "../i18n";

export function QuickSwitcher() {
  const t = useT();
  const projects = useStore((s) => s.projects);
  const openIds = useStore((s) => s.workspace.openProjectIds);
  const sessions = useStore((s) => s.sessions);
  const status = useStore((s) => s.status);
  const set = useStore((s) => s.set);
  const activate = useStore((s) => s.activate);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => inputRef.current?.focus(), []);

  const results = useMemo(() => {
    const scored = projects
      .map((p) => ({
        p,
        score: matchScore(query, [
          p.name,
          p.description,
          p.tags.join(" "),
          p.summary?.text ?? "",
          p.path,
        ]),
        activity: lastActivity({ sessions }, p.id) ?? 0,
      }))
      .filter((r) => r.score >= 0);
    scored.sort((a, b) =>
      query ? b.score - a.score || b.activity - a.activity : b.activity - a.activity,
    );
    return scored.slice(0, 50);
  }, [projects, query, sessions]);

  const close = () => set({ switcherOpen: false });
  const choose = (id: string) => {
    activate(id);
    close();
  };

  return (
    <div className="overlay" onMouseDown={close}>
      <div className="switcher" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          value={query}
          placeholder={t("switcher.placeholder")}
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
              setSel((i) => Math.min(i + 1, results.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSel((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter" && results[sel]) {
              choose(results[sel].p.id);
            }
          }}
        />
        <ul className="switcher__list">
          {results.map((r, i) => (
            <li
              key={r.p.id}
              className={i === sel ? "is-selected" : ""}
              onMouseEnter={() => setSel(i)}
              onClick={() => choose(r.p.id)}
            >
              <span className="row__color" style={{ background: r.p.color }} />
              <StatusDot
                status={openIds.includes(r.p.id) ? projectStatus({ status }, r.p.id) : undefined}
              />
              <span className="switcher__name">{r.p.name}</span>
              <span className="switcher__desc">
                {r.p.description || r.p.summary?.text || r.p.path}
              </span>
              <span className="row__time">{relativeTime(r.activity || undefined, t)}</span>
            </li>
          ))}
          {results.length === 0 && <li className="muted">{t("switcher.none")}</li>}
        </ul>
      </div>
    </div>
  );
}
