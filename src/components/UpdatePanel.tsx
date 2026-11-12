import { useT } from "../i18n";
import { flushDocuments } from "../store";
import { useUpdater } from "../updater";

/** Update section of the settings dialog. */
export function UpdatePanel() {
  const t = useT();
  const phase = useUpdater((s) => s.phase);
  const current = useUpdater((s) => s.currentVersion);
  const check = useUpdater((s) => s.check);
  const download = useUpdater((s) => s.download);
  const install = useUpdater((s) => s.install);
  const version = phase.version ?? "";

  const status = (() => {
    switch (phase.status) {
      case "dev":
        return t("update.dev");
      case "checking":
        return t("update.checking");
      case "uptodate":
        return t("update.upToDate");
      case "available":
        return t("update.available", { version });
      case "downloading":
        return t("update.downloading", { percent: Math.round(phase.percent ?? 0) });
      case "ready":
        return t("update.ready", { version });
      case "installing":
        return t("update.installing");
      case "error":
        return t("update.error");
      default:
        return "";
    }
  })();

  const canCheck =
    phase.status === "idle" ||
    phase.status === "uptodate" ||
    phase.status === "error" ||
    phase.status === "dev";
  const showNotes =
    !!phase.notes &&
    (phase.status === "available" || phase.status === "ready" || phase.status === "downloading");

  return (
    <section className="update">
      <div className="update__row">
        <span className="field__label">{t("update.title")}</span>
        <span className="update__version">{t("update.current", { version: current })}</span>
        <span className={`update__status update__status--${phase.status}`}>{status}</span>
        <span className="spacer" />
        {canCheck && (
          <button className="btn btn--small" onClick={() => void check()}>
            {t("update.check")}
          </button>
        )}
        {phase.status === "available" && (
          <button className="btn btn--small btn--primary" onClick={() => void download()}>
            {t("update.download")}
          </button>
        )}
        {phase.status === "ready" && (
          <button
            className="btn btn--small btn--primary"
            onClick={() => void install(flushDocuments)}
            title={t("update.installTitle")}
          >
            {t("update.install")}
          </button>
        )}
      </div>
      {phase.status === "downloading" && (
        <div className="update__bar">
          <div style={{ width: `${phase.percent ?? 0}%` }} />
        </div>
      )}
      {phase.status === "ready" && <p className="field__hint">{t("update.readyHint")}</p>}
      {phase.error && <p className="error">{phase.error}</p>}
      {showNotes && (
        <details className="update__notes">
          <summary>{t("update.notes")}</summary>
          <pre>{phase.notes}</pre>
        </details>
      )}
    </section>
  );
}
