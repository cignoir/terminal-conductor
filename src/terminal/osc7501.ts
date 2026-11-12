// OSC 7501 ("program status"): AI CLIs report their state to the terminal.
// Observed from Codex CLI 0.167.2 and Antigravity CLI 1.5.5:
//
//   ESC ] 7501 ; state=working:app=codex BEL
//   ESC ] 7501 ; state=blocked:kind=permission:app=antigravity:msg=<base64> BEL
//   ESC ] 7501 ; state=done:app=antigravity:msg=<base64 of the last reply> BEL
//
// Fields are key=value pairs separated by ':'; msg is base64-encoded UTF-8.
// States seen: idle, working, blocked (kind=permission|auth|...), done, error.

import type { ClaudeStatus } from "../types";

export type ProgramStatus = {
  state: string;
  kind?: string;
  app?: string;
  msg?: string;
};

export function parseOsc7501(data: string): ProgramStatus | null {
  const fields: Record<string, string> = {};
  for (const part of data.split(":")) {
    const eq = part.indexOf("=");
    if (eq > 0) fields[part.slice(0, eq)] = part.slice(eq + 1);
  }
  if (!fields.state) return null;
  return {
    state: fields.state,
    kind: fields.kind,
    app: fields.app,
    msg: fields.msg ? decodeBase64Utf8(fields.msg) : undefined,
  };
}

function decodeBase64Utf8(b64: string): string | undefined {
  try {
    const bin = atob(b64);
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return undefined;
  }
}

/** Maps a program state to the app's status; undefined means "ignore". */
export function statusFromProgram(p: ProgramStatus): ClaudeStatus | undefined {
  switch (p.state) {
    case "working":
      return "working";
    case "blocked":
      return "needs_permission";
    case "done":
      return "done";
    case "error":
    case "failed":
      return "error";
    case "idle":
      return "idle";
    default:
      return undefined;
  }
}
