import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { getTerminal } from "../terminal/cache";
import { useT } from "../i18n";
import { History, MessageSquarePlus, Play, RotateCcw, X } from "lucide-react";
import { IconButton } from "./IconButton";
import {
  type LaunchMode,
  type PaneRole,
  type Project,
  type ResumeInfo,
  type Settings,
  paneId,
} from "../types";

/** Ask a mounted pane to kill its process and start again. */
export function requestRestart(id: string, fresh: boolean) {
  window.dispatchEvent(new CustomEvent("tc-restart", { detail: { id, fresh } }));
}

/**
 * Whether and how to start Claude Code when a pane starts. Claude only starts
 * by itself when the user turned that on; otherwise the pane is a plain shell
 * and offers a Resume button. `fresh` is an explicit "new conversation".
 */
export function launchModeFor(
  project: Project,
  role: PaneRole,
  settings: Settings,
  fresh = false,
): LaunchMode | undefined {
  if (role !== "claude") return undefined;
  if (fresh) return "new";
  if (!settings.autoStartClaude) return undefined;
  return project.continueOnFirstOpen ? "resumeOrContinue" : "resume";
}

/** Resume offers by pane, so they survive switching projects. */
const offers = new Map<string, ResumeInfo | null>();

async function waitForExit(id: string) {
  for (let i = 0; i < 40; i++) {
    if (!(await api.alive()).includes(id)) return;
    await new Promise((r) => setTimeout(r, 100));
  }
}

type Props = {
  project: Project;
  role: PaneRole;
  focused: boolean;
  onFocus: () => void;
};

export function TerminalPane({ project, role, focused, onFocus }: Props) {
  const t = useT();
  const hostRef = useRef<HTMLDivElement>(null);
  const id = paneId(project.id, role);
  const alive = useStore((s) => s.alive[id]);
  const settings = useStore((s) => s.settings);
  const setAlive = useStore((s) => s.setAlive);
  const [error, setError] = useState<string | null>(null);
  const [offer, setOfferState] = useState<ResumeInfo | null>(() => offers.get(id) ?? null);
  const setOffer = (o: ResumeInfo | null) => {
    offers.set(id, o);
    setOfferState(o);
  };
  // Hide the Resume offer once Claude reports in (e.g. typed by hand).
  const claudeRunning = useStore((s) => s.status[id] !== undefined && s.status[id] !== "exited");
  const shell = project.shell ?? settings.defaultShell;

  const start = async (fresh = false) => {
    const entry = getTerminal(id, useStore.getState().settings);
    setError(null);
    try {
      await entry.attach();
      const { cols, rows } = entry.size;
      const launch = launchModeFor(project, role, useStore.getState().settings, fresh);
      const spawned = await api.ensure({ paneId: id, cwd: project.path, shell, cols, rows, launch });
      // Continuing is a one-time migration step; don't try it again.
      if (spawned && launch === "resumeOrContinue") {
        useStore.getState().updateProject(project.id, { continueOnFirstOpen: false });
      }
      if (spawned || !useStore.getState().alive[id]) setAlive(id, true);
      if (spawned) await api.resize(id, entry.size.cols, entry.size.rows);
      if (spawned) {
        setOffer(
          role === "claude" && !launch
            ? await api.claudeResumeInfo(id, project.path).catch(() => null)
            : null,
        );
      }
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const entry = getTerminal(id, useStore.getState().settings);
    entry.mount(host);
    void start();

    let frame = 0;
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => entry.fitNow());
    });
    ro.observe(host);
    const onFocusIn = () => onFocus();
    entry.term.textarea?.addEventListener("focus", onFocusIn);
    return () => {
      ro.disconnect();
      cancelAnimationFrame(frame);
      entry.term.textarea?.removeEventListener("focus", onFocusIn);
      entry.unmount();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, project.path, shell]);

  useEffect(() => {
    const onRestart = (e: Event) => {
      const d = (e as CustomEvent<{ id: string; fresh: boolean }>).detail;
      if (d.id !== id) return;
      void (async () => {
        await api.kill(id);
        await waitForExit(id);
        getTerminal(id, useStore.getState().settings).term.reset();
        await start(d.fresh);
      })();
    };
    window.addEventListener("tc-restart", onRestart);
    return () => window.removeEventListener("tc-restart", onRestart);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, project.path, shell]);

  useEffect(() => {
    const onDismiss = (e: Event) => {
      if ((e as CustomEvent<string>).detail === id) setOffer(null);
    };
    window.addEventListener("tc-offer-dismiss", onDismiss);
    return () => window.removeEventListener("tc-offer-dismiss", onDismiss);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  useEffect(() => {
    if (focused) getTerminal(id, useStore.getState().settings).term.focus();
  }, [focused, id]);

  const startClaude = (mode: LaunchMode) => {
    setOffer(null);
    void api.claudeLaunch(id, project.path, mode).catch((e) => setError(String(e)));
    getTerminal(id, useStore.getState().settings).term.focus();
  };
  const showOffer =
    !!offer && (!!offer.sessionId || offer.canContinue) && !claudeRunning && alive !== false;

  return (
    <div className={`pane ${focused ? "pane--focused" : ""}`}>
      <div className="pane__host" ref={hostRef} />
      {showOffer && (
        <div className="pane__bar pane__bar--offer">
          <span>{offer.sessionId ? t("pane.resumeFound") : t("pane.continueFound")}</span>
          <IconButton
            icon={offer.sessionId ? Play : History}
            variant="primary"
            label={offer.sessionId ? t("pane.resumeTip") : t("pane.continueTip")}
            onClick={() => startClaude(offer.sessionId ? "resume" : "continue")}
          />
          <IconButton icon={MessageSquarePlus} label={t("pane.newClaudeTip")} onClick={() => startClaude("new")} />
          <IconButton icon={X} label={t("pane.dismiss")} onClick={() => setOffer(null)} />
        </div>
      )}
      {(alive === false || error) && (
        <div className="pane__bar">
          <span>{error ?? t("pane.exited")}</span>
          <IconButton icon={RotateCcw} variant="primary" label={t("pane.restart")} onClick={() => void start()} />
          {role === "claude" && (
            <IconButton
              icon={MessageSquarePlus}
              label={t("pane.restartFresh")}
              onClick={() => requestRestart(id, true)}
            />
          )}
        </div>
      )}
    </div>
  );
}
