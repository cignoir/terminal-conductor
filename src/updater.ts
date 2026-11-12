// Auto-update state. Rust commands updater_check / updater_download /
// updater_install plus update:progress / update:downloaded events.
//
// Download and install are separate: installing exits the app (Windows runs
// the installer), so it only happens when the user presses the button, after
// documents have been saved.

import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

export type UpdateStatus =
  | "idle"
  | "dev"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "installing"
  | "uptodate"
  | "error";

export type UpdatePhase = {
  status: UpdateStatus;
  version?: string;
  notes?: string;
  percent?: number;
  /** The last action's failure, shown next to the retry button. */
  error?: string;
};

type CheckResult = {
  available: boolean;
  dev?: boolean;
  version?: string;
  notes?: string | null;
};

/** Check once this long after startup, quietly. */
const STARTUP_CHECK_DELAY_MS = 5000;
/** Long-running sessions check again this often. */
const PERIODIC_CHECK_MS = 6 * 60 * 60 * 1000;

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

function phaseFrom(res: CheckResult): UpdatePhase {
  if (res.dev) return { status: "dev" };
  if (!res.available) return { status: "uptodate" };
  return { status: "available", version: res.version, notes: res.notes ?? undefined };
}

type UpdaterStore = {
  phase: UpdatePhase;
  currentVersion: string;
  check: () => Promise<void>;
  download: () => Promise<void>;
  install: (beforeInstall: () => Promise<void>) => Promise<void>;
};

export const useUpdater = create<UpdaterStore>((set, get) => ({
  phase: { status: "idle" },
  currentVersion: "",

  check: async () => {
    set({ phase: { status: "checking" } });
    try {
      set({ phase: phaseFrom(await invoke<CheckResult>("updater_check")) });
    } catch (e) {
      set({ phase: { status: "error", error: errorText(e) } });
    }
  },

  download: async () => {
    set((s) => ({ phase: { ...s.phase, status: "downloading", percent: 0, error: undefined } }));
    try {
      await invoke("updater_download");
      set((s) => ({ phase: { ...s.phase, status: "ready", error: undefined } }));
    } catch (e) {
      // The found update stays in Rust, so the download can be retried.
      set((s) => ({ phase: { ...s.phase, status: "available", error: errorText(e) } }));
    }
  },

  install: async (beforeInstall) => {
    if (get().phase.status !== "ready") return;
    set((s) => ({ phase: { ...s.phase, status: "installing", error: undefined } }));
    try {
      await beforeInstall();
      // On success the app exits; this never returns.
      await invoke("updater_install");
    } catch (e) {
      set((s) => ({ phase: { ...s.phase, status: "ready", error: errorText(e) } }));
    }
  },
}));

/** True when there is something for the user to act on. */
export function updateNeedsAttention(p: UpdatePhase) {
  return p.status === "available" || p.status === "ready";
}

let started = false;

/** Starts background checks and event listeners. Call once at startup. */
export function startUpdater() {
  if (started) return;
  started = true;
  getVersion()
    .then((v) => useUpdater.setState({ currentVersion: v }))
    .catch(() => {});

  void listen<{ percent: number }>("update:progress", (e) => {
    useUpdater.setState((s) => ({
      phase: { ...s.phase, status: "downloading", percent: e.payload.percent },
    }));
  });
  void listen("update:downloaded", () => {
    useUpdater.setState((s) => ({ phase: { ...s.phase, status: "ready", error: undefined } }));
  });

  // Quiet checks: only surface a found update (or dev mode); ignore failures
  // and "up to date", and never interrupt a download in progress.
  const quietCheck = () => {
    const { status } = useUpdater.getState().phase;
    if (status === "downloading" || status === "ready" || status === "installing") return;
    invoke<CheckResult>("updater_check")
      .then((res) => {
        if (res?.available || res?.dev) useUpdater.setState({ phase: phaseFrom(res) });
      })
      .catch(() => {});
  };
  window.setTimeout(quietCheck, STARTUP_CHECK_DELAY_MS);
  window.setInterval(quietCheck, PERIODIC_CHECK_MS);
}
