import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import "@/i18n";
import "./styles.css";

type LearnedPair = [string, string];

const FixEditor: React.FC = () => {
  const { t } = useTranslation();
  const [initialText, setInitialText] = React.useState<string | null>(null);
  const [editedText, setEditedText] = React.useState("");
  const [learned, setLearned] = React.useState<LearnedPair[]>([]);
  const [guarded, setGuarded] = React.useState(false);
  const [saved, setSaved] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [error, setError] = React.useState(false);
  const [correctedCopy, setCorrectedCopy] = React.useState("");
  const [copied, setCopied] = React.useState(false);
  const [closing, setClosing] = React.useState(false);
  const closeTimerRef = React.useRef<number | null>(null);

  const clearPendingClose = React.useCallback(() => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  }, []);

  const resetEditor = React.useCallback(
    (text: string | null) => {
      clearPendingClose();
      setInitialText(text);
      setEditedText(text ?? "");
      setCorrectedCopy(text ?? "");
      setLearned([]);
      setGuarded(false);
      setSaved(false);
      setSaving(false);
      setError(false);
      setCopied(false);
      setClosing(false);
    },
    [clearPendingClose],
  );

  React.useEffect(() => {
    let cancelled = false;

    const loadLastTranscript = async () => {
      const result = await commands.getLastTranscript();
      if (cancelled) return;
      const text = result.status === "ok" ? result.data : null;
      resetEditor(text);
    };

    void loadLastTranscript();

    let unlistenRefresh: (() => void) | undefined;
    getCurrentWebviewWindow()
      .listen("fix-editor-refresh", () => {
        void loadLastTranscript();
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
          return;
        }
        unlistenRefresh = unlisten;
      })
      .catch(console.error);

    return () => {
      cancelled = true;
      clearPendingClose();
      unlistenRefresh?.();
    };
  }, [clearPendingClose, resetEditor]);

  const save = async () => {
    clearPendingClose();
    setSaving(true);
    setError(false);
    setSaved(false);
    setCopied(false);
    setClosing(false);
    const result = await commands.saveTranscriptFix(editedText);
    setSaving(false);
    if (result.status === "error") {
      setError(true);
      return;
    }
    setLearned(result.data.learned as LearnedPair[]);
    setGuarded(result.data.rejected_guard);
    setCorrectedCopy(result.data.corrected_copy || editedText);
    setSaved(true);
    if (!result.data.rejected_guard) {
      setClosing(true);
      closeTimerRef.current = window.setTimeout(() => {
        closeTimerRef.current = null;
        void getCurrentWebviewWindow().close().catch(console.error);
      }, 1200);
    }
  };

  const copyCorrected = async () => {
    await writeText(correctedCopy || editedText);
    setCopied(true);
  };

  if (initialText === null) {
    return (
      <main className="fix-shell empty">
        <p>{t("fixEditor.empty")}</p>
      </main>
    );
  }

  return (
    <main className="fix-shell">
      <header>
        <h1>{t("fixEditor.title")}</h1>
      </header>
      <label className="editor-label" htmlFor="fix-editor-textarea">
        {t("fixEditor.textareaLabel")}
      </label>
      <textarea
        id="fix-editor-textarea"
        value={editedText}
        onChange={(event) => {
          setEditedText(event.target.value);
          setSaved(false);
          setCopied(false);
        }}
        autoFocus
      />
      <section className="status" aria-live="polite">
        {guarded && <p className="guard">{t("fixEditor.rewriteGuard")}</p>}
        {!guarded &&
          learned.map(([heard, correct]) => (
            <p className="learned" key={`${heard}:${correct}`}>
              {t("fixEditor.changed", { heard, correct })}
            </p>
          ))}
        {saved && !guarded && learned.length === 0 && (
          <p className="muted">{t("fixEditor.nothingLearned")}</p>
        )}
        {saved && learned.length > 0 && (
          <p className="saved">{t("fixEditor.saved")}</p>
        )}
        {copied && <p className="saved">{t("fixEditor.copyReady")}</p>}
        {error && <p className="error">{t("fixEditor.error")}</p>}
      </section>
      <footer>
        <button
          type="button"
          className="secondary"
          onClick={copyCorrected}
          disabled={saving || closing}
        >
          {t("fixEditor.copyCorrected")}
        </button>
        <button
          type="button"
          className="primary"
          onClick={save}
          disabled={saving || closing}
        >
          {saving ? t("fixEditor.saving") : t("fixEditor.save")}
        </button>
      </footer>
    </main>
  );
};

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <FixEditor />
  </React.StrictMode>,
);
