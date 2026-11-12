import { Channel } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal, type ITheme } from "@xterm/xterm";
import { api } from "../api";
import { useStore } from "../store";
import type { Settings } from "../types";

/** How many xterm instances to keep alive while hidden. */
const MAX_CACHED = 8;

export const THEME: ITheme = {
  background: "#0f1115",
  foreground: "#d6d9df",
  cursor: "#d6d9df",
  cursorAccent: "#0f1115",
  selectionBackground: "#3a4a66",
  black: "#1c1f26",
  red: "#e5675b",
  green: "#7cc382",
  yellow: "#e2b955",
  blue: "#6aa2f0",
  magenta: "#c08ad8",
  cyan: "#5cc3cf",
  white: "#c8ccd4",
  brightBlack: "#5c6370",
  brightRed: "#ff7e72",
  brightGreen: "#97d99c",
  brightYellow: "#f0cd72",
  brightBlue: "#88b8ff",
  brightMagenta: "#d6a6ec",
  brightCyan: "#7dd8e2",
  brightWhite: "#eef0f4",
};

/** Keys the app handles itself instead of sending to the shell. */
export function isAppShortcut(e: KeyboardEvent): boolean {
  if (e.ctrlKey && !e.altKey && (e.key === "Tab" || e.key === "k" || e.key === "K"))
    return true;
  if (e.ctrlKey && !e.altKey && !e.shiftKey && /^[1-9]$/.test(e.key)) return true;
  if (e.altKey && !e.ctrlKey && (e.key === "ArrowUp" || e.key === "ArrowDown"))
    return true;
  if (e.ctrlKey && e.key === ",") return true;
  if (e.ctrlKey && e.shiftKey && (e.key === "F" || e.key === "f")) return true;
  return false;
}

class TermEntry {
  readonly term: Terminal;
  readonly el: HTMLDivElement;
  private fit = new FitAddon();
  readonly search = new SearchAddon();
  private webgl: WebglAddon | null = null;
  private opened = false;
  private attachId: number | null = null;
  private suppressInput = true;
  private parsed = 0;
  private replayLen: number | null = null;
  private attaching: Promise<void> | null = null;

  constructor(readonly paneId: string, settings: Settings) {
    this.el = document.createElement("div");
    this.el.className = "xterm-host";
    this.term = new Terminal({
      fontFamily: settings.fontFamily,
      fontSize: settings.fontSize,
      scrollback: settings.scrollback,
      allowProposedApi: true,
      cursorBlink: true,
      theme: THEME,
      windowsPty: { backend: "conpty", buildNumber: 22621 },
      rightClickSelectsWord: false,
    });
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.search);
    const unicode = new Unicode11Addon();
    this.term.loadAddon(unicode);
    this.term.unicode.activeVersion = "11";
    this.term.loadAddon(
      new WebLinksAddon((_e, uri) => {
        void openUrl(uri);
      }),
    );

    // OSC 7501 status reports are read by the backend from the PTY stream
    // (so panes without a live terminal are covered); just swallow them here.
    this.term.parser.registerOscHandler(7501, () => true);

    this.term.onData((data) => {
      // Replayed output may contain queries (e.g. cursor position) that
      // xterm answers; those answers belong to the past, so drop them.
      if (this.suppressInput) return;
      void api.write(this.paneId, data);
      // Answering a permission prompt means Claude is working again. No hook
      // fires for that until the tool finishes, so update the badge here.
      // Focus reports (ESC [ I / ESC [ O) are not answers.
      if (data !== "[I" && data !== "[O") {
        const s = useStore.getState();
        if (s.status[this.paneId] === "needs_permission") {
          s.set({ status: { ...s.status, [this.paneId]: "working" } });
        }
      }
    });
    this.term.onBinary((data) => {
      if (this.suppressInput) return;
      void api.write(this.paneId, data);
    });
    this.term.onResize(({ cols, rows }) => {
      void api.resize(this.paneId, cols, rows).catch(() => {});
    });
    this.term.attachCustomKeyEventHandler((e) => this.onKey(e));
    this.el.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      // When the program has turned on mouse reporting (Claude Code does),
      // xterm already sends it the right click, and Claude Code pastes on
      // its own; pasting here too would paste twice. Shift+right-click
      // bypasses mouse reporting, so it stays ours.
      if (this.term.modes.mouseTrackingMode !== "none" && !e.shiftKey) return;
      if (this.term.hasSelection()) {
        void writeText(this.term.getSelection());
        this.term.clearSelection();
      } else {
        void this.pasteFromClipboard();
      }
    });
  }

  private async pasteFromClipboard() {
    const text = await readText().catch(() => "");
    if (text) this.term.paste(text.replace(/\r?\n/g, "\r"));
    this.term.focus();
  }

  private onKey(e: KeyboardEvent): boolean {
    if (e.type !== "keydown") return true;
    if (isAppShortcut(e)) return false;
    const key = e.key.toLowerCase();
    // Copy: Ctrl+Shift+C always, Ctrl+C only when text is selected.
    if (e.ctrlKey && key === "c" && (e.shiftKey || this.term.hasSelection())) {
      if (this.term.hasSelection()) {
        void writeText(this.term.getSelection());
        this.term.clearSelection();
      }
      e.preventDefault();
      return false;
    }
    if (e.ctrlKey && !e.altKey && key === "v") {
      e.preventDefault();
      void this.pasteFromClipboard();
      return false;
    }
    // Shift+Enter inserts a newline in Claude Code's prompt (sent as Alt+Enter).
    if (e.shiftKey && !e.ctrlKey && !e.altKey && e.key === "Enter") {
      e.preventDefault();
      void api.write(this.paneId, "\x1b\r");
      return false;
    }
    return true;
  }

  /** Subscribe to the pane's output. Safe to call repeatedly. */
  attach(): Promise<void> {
    if (this.attachId !== null) return Promise.resolve();
    if (this.attaching) return this.attaching;
    this.suppressInput = true;
    this.parsed = 0;
    this.replayLen = null;
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = (msg) => {
      const bytes =
        msg instanceof ArrayBuffer
          ? new Uint8Array(msg)
          : new Uint8Array(msg as unknown as number[]);
      this.term.write(bytes, () => {
        this.parsed += bytes.length;
        this.checkReplayDone();
      });
    };
    this.attaching = api.attach(this.paneId, channel).then((r) => {
      this.attachId = r.attachId;
      this.replayLen = r.replayed;
      this.checkReplayDone();
      this.attaching = null;
    });
    return this.attaching;
  }

  private checkReplayDone() {
    if (this.replayLen !== null && this.parsed >= this.replayLen) {
      this.suppressInput = false;
    }
  }

  mount(host: HTMLElement) {
    if (this.el.parentElement !== host) host.appendChild(this.el);
    if (!this.opened) {
      this.term.open(this.el);
      this.opened = true;
    }
    if (!this.webgl) {
      try {
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => {
          webgl.dispose();
          if (this.webgl === webgl) this.webgl = null;
        });
        this.term.loadAddon(webgl);
        this.webgl = webgl;
      } catch {
        this.webgl = null;
      }
    }
    this.fitNow();
  }

  unmount() {
    // Free the WebGL context while hidden; the DOM renderer takes over.
    this.webgl?.dispose();
    this.webgl = null;
    this.el.remove();
  }

  fitNow() {
    if (!this.opened || !this.el.isConnected) return;
    if (this.el.clientWidth < 20 || this.el.clientHeight < 20) return;
    try {
      this.fit.fit();
    } catch {
      /* not measurable yet */
    }
  }

  get size() {
    return { cols: this.term.cols, rows: this.term.rows };
  }

  applySettings(s: Settings) {
    this.term.options.fontFamily = s.fontFamily;
    this.term.options.fontSize = s.fontSize;
    this.term.options.scrollback = s.scrollback;
    this.fitNow();
  }

  dispose() {
    if (this.attachId !== null) void api.detach(this.paneId, this.attachId);
    this.webgl?.dispose();
    this.term.dispose();
    this.el.remove();
  }
}

const entries = new Map<string, TermEntry>();

export function getTerminal(paneId: string, settings: Settings): TermEntry {
  let e = entries.get(paneId);
  if (e) {
    // Move to most-recently-used.
    entries.delete(paneId);
    entries.set(paneId, e);
    return e;
  }
  e = new TermEntry(paneId, settings);
  entries.set(paneId, e);
  evict();
  return e;
}

function evict() {
  for (const [id, e] of entries) {
    if (entries.size <= MAX_CACHED) break;
    if (e.el.isConnected) continue;
    e.dispose();
    entries.delete(id);
  }
}

export function disposeTerminal(paneId: string) {
  entries.get(paneId)?.dispose();
  entries.delete(paneId);
}

export function applySettingsToAll(s: Settings) {
  for (const e of entries.values()) e.applySettings(s);
}

export function focusTerminal(paneId: string) {
  entries.get(paneId)?.term.focus();
}

export function clearTerminal(paneId: string) {
  entries.get(paneId)?.term.clear();
}

export type { TermEntry };
