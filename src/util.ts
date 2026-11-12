import type { Translate } from "./i18n";

export function relativeTime(
  ms: number | undefined,
  t: Translate,
  now = Date.now(),
): string {
  if (!ms) return "";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return t("time.now");
  const m = Math.round(s / 60);
  if (m < 60) return t("time.minutes", { n: m });
  const h = Math.round(m / 60);
  if (h < 24) return t("time.hours", { n: h });
  const d = Math.round(h / 24);
  if (d < 30) return t("time.days", { n: d });
  const mo = Math.round(d / 30);
  if (mo < 12) return t("time.months", { n: mo });
  return t("time.years", { n: Math.round(mo / 12) });
}

/**
 * Scores how well `query` matches `text`: every query token must appear.
 * Returns -1 for no match; higher is better.
 */
export function matchScore(query: string, fields: string[]): number {
  const tokens = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return 0;
  const hay = fields.map((f) => f.toLowerCase());
  let score = 0;
  for (const t of tokens) {
    let best = -1;
    hay.forEach((h, i) => {
      const at = h.indexOf(t);
      if (at < 0) return;
      // Earlier fields (name) and earlier positions weigh more.
      const s = 100 - i * 20 - Math.min(at, 30) + (at === 0 ? 20 : 0);
      if (s > best) best = s;
    });
    if (best < 0) return -1;
    score += best;
  }
  return score;
}

export function firstLine(text: string | undefined, max = 120): string {
  if (!text) return "";
  const line = text.split(/\r?\n/).find((l) => l.trim()) ?? "";
  const clean = line.replace(/[#*`>_]/g, "").trim();
  return clean.length > max ? `${clean.slice(0, max)}…` : clean;
}
