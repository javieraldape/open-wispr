//! Tauri commands for the E1 one-tap fix flow.
//!
//! The pure extraction/apply logic lives in `crate::corrections` (Tauri-free);
//! these commands are the thin app-context seam that wires it to the last
//! transcript, the store, and the Fix editor window.

use std::sync::{Arc, Mutex};

use log::warn;
use once_cell::sync::Lazy;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindowBuilder};

use crate::corrections::{extract, RewriteGuard};
use crate::managers::corrections::{apply_corrections, CorrectionsManager, ReversalOutcome};
use crate::managers::history::HistoryManager;

static LAST_PASTED_TRANSCRIPT: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

pub(crate) fn set_last_pasted_transcript(text: String) {
    if text.trim().is_empty() {
        return;
    }
    if let Ok(mut last_pasted) = LAST_PASTED_TRANSCRIPT.lock() {
        *last_pasted = Some(text);
    }
}

fn cached_last_pasted_transcript() -> Option<String> {
    LAST_PASTED_TRANSCRIPT
        .lock()
        .ok()
        .and_then(|last_pasted| last_pasted.clone())
}

fn choose_fix_transcript(
    last_pasted: Option<String>,
    history_transcript: Option<String>,
) -> Option<String> {
    last_pasted
        .filter(|text| !text.trim().is_empty())
        .or_else(|| history_transcript.filter(|text| !text.trim().is_empty()))
}

/// The last completed transcript text (post-processed if present, else raw),
/// or `None` when there is no completed entry or it is empty/whitespace-only.
fn last_transcript(history_manager: &HistoryManager) -> Result<Option<String>, String> {
    let history_transcript = history_manager
        .get_latest_completed_entry()
        .map_err(|e| e.to_string())?
        .map(|entry| crate::tray::last_transcript_text(&entry).to_string());

    Ok(choose_fix_transcript(
        cached_last_pasted_transcript(),
        history_transcript,
    ))
}

/// Fetch the last transcript to seed the Fix editor. `None` when nothing to fix.
#[tauri::command]
#[specta::specta]
pub async fn get_last_transcript(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
) -> Result<Option<String>, String> {
    last_transcript(&history_manager)
}

/// The outcome of saving a one-tap fix: which pairs were learned, whether the
/// whole edit tripped the rewrite guard, and the shown text re-corrected with
/// the freshly learned pairs (for the "copy to clipboard" teach-only affordance
/// — E1 never re-pastes into the target app).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SaveFixResult {
    /// (heard, correct) pairs actually learned (post-reversal, post-rejection).
    pub learned: Vec<(String, String)>,
    /// True iff the edit was discarded as a rewrite (>50% of tokens changed).
    pub rejected_guard: bool,
    /// The shown transcript with all corrections (incl. the just-learned ones)
    /// applied — the text the user can copy. Empty when there was nothing to fix.
    pub corrected_copy: String,
}

/// Save a one-tap fix: extract candidate pairs from the edit, learn them (with
/// the reversal rule), and return the re-corrected copy. Teach-only — never
/// re-inserts text into the target app.
#[tauri::command]
#[specta::specta]
pub async fn save_transcript_fix(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    corrections_manager: State<'_, Arc<CorrectionsManager>>,
    edited_text: String,
) -> Result<SaveFixResult, String> {
    let Some(shown_text) = last_transcript(&history_manager)? else {
        // Nothing to fix → no-op (matches the plan's E1 empty/zero-diff case).
        return Ok(SaveFixResult {
            learned: Vec::new(),
            rejected_guard: false,
            corrected_copy: String::new(),
        });
    };

    let outcome = extract::extract_corrections(&shown_text, &edited_text);
    let rejected_guard = matches!(outcome.rejected_reason, Some(RewriteGuard::TooMuchChanged));

    let mut learned: Vec<(String, String)> = Vec::new();
    for pair in &outcome.pairs {
        match corrections_manager.learn_or_reverse(&pair.heard, &pair.correct) {
            Ok(ReversalOutcome::Learned(_)) => {
                learned.push((pair.heard.clone(), pair.correct.clone()));
            }
            // A reversal deleted the inverse pair — nothing was learned, so it
            // is not reported in `learned`.
            Ok(ReversalOutcome::Reversed(_)) => {}
            Err(e) => {
                // A single bad candidate (e.g. rejected by the store's own
                // validation) must not fail the whole command.
                warn!("Skipping unlearnable fix candidate {pair:?}: {e}");
            }
        }
    }

    // Recompute the corrected copy AFTER learning so it reflects the fresh pairs.
    let corrected_copy = apply_corrections(&corrections_manager, &shown_text);

    Ok(SaveFixResult {
        learned,
        rejected_guard,
        corrected_copy,
    })
}

/// A pure preview of what a fix would learn — for the live "Changed: X → Y"
/// panel as the user types. No state, no side effects (frontend debounces).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PreviewFixResult {
    pub pairs: Vec<(String, String)>,
    pub rewrite_guard: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn preview_fix(
    shown_text: String,
    edited_text: String,
) -> Result<PreviewFixResult, String> {
    let outcome = extract::extract_corrections(&shown_text, &edited_text);
    Ok(PreviewFixResult {
        pairs: outcome
            .pairs
            .into_iter()
            .map(|p| (p.heard, p.correct))
            .collect(),
        rewrite_guard: matches!(outcome.rejected_reason, Some(RewriteGuard::TooMuchChanged)),
    })
}

/// A serde+specta DTO mirroring `corrections::store::StoredCorrection` — the
/// store stays Tauri/serde-free, so conversion happens here at the seam.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct StoredCorrectionDto {
    pub id: i64,
    pub heard_key: String,
    pub heard_text: String,
    pub correct_text: String,
    /// "learned" or "manual".
    pub source: String,
    pub verbatim: bool,
    pub created_at: String,
    pub last_applied_at: Option<String>,
}

/// All stored corrections, newest first (the store already sorts; do not
/// re-sort on the frontend).
#[tauri::command]
#[specta::specta]
pub async fn corrections_list(
    corrections_manager: State<'_, Arc<CorrectionsManager>>,
) -> Result<Vec<StoredCorrectionDto>, String> {
    let rows = corrections_manager.list().map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| StoredCorrectionDto {
            id: r.id,
            heard_key: r.heard_key,
            heard_text: r.heard_text,
            correct_text: r.correct_text,
            source: r.source.as_str().to_string(),
            verbatim: r.verbatim,
            created_at: r.created_at,
            last_applied_at: r.last_applied_at,
        })
        .collect())
}

/// Delete a stored correction by its heard_key. Returns whether it existed.
#[tauri::command]
#[specta::specta]
pub async fn corrections_delete(
    corrections_manager: State<'_, Arc<CorrectionsManager>>,
    heard_key: String,
) -> Result<bool, String> {
    corrections_manager
        .delete(&heard_key)
        .map_err(|e| e.to_string())
}

/// Manually add or update a correction pair from the settings panel.
#[tauri::command]
#[specta::specta]
pub async fn corrections_add_manual(
    corrections_manager: State<'_, Arc<CorrectionsManager>>,
    heard_text: String,
    correct_text: String,
    verbatim: bool,
) -> Result<String, String> {
    corrections_manager
        .add_manual(&heard_text, &correct_text, verbatim)
        .map_err(|e| e.to_string())
}

/// The Fix editor window label and its dedicated Vite entry HTML.
const FIX_EDITOR_LABEL: &str = "fix_editor";
const FIX_EDITOR_URL: &str = "src/fix-editor/index.html";

/// Create (or reveal) the Fix editor window. A SEPARATE WebviewWindow — NOT
/// the NSPanel recording overlay — because the fix editor is a real, decorated,
/// focusable window with its own lifecycle. Callable as a plain function from
/// the tray menu and the ⌥⌘F global shortcut (which have `&AppHandle`, not the
/// command-invocation context).
pub fn open_fix_editor_window(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(FIX_EDITOR_LABEL) {
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        let _ = win.emit("fix-editor-refresh", ());
        return Ok(());
    }

    let win = WebviewWindowBuilder::new(
        app,
        FIX_EDITOR_LABEL,
        tauri::WebviewUrl::App(FIX_EDITOR_URL.into()),
    )
    .title("Fix Last Transcript")
    .inner_size(480.0, 460.0)
    .min_inner_size(420.0, 380.0)
    .resizable(true)
    .maximizable(false)
    .decorations(true)
    .focused(true)
    .center()
    .build()
    .map_err(|e| e.to_string())?;

    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

/// Tauri command wrapper — the overlay's Edit button and any frontend caller
/// invoke this; it delegates to the shared `open_fix_editor_window`.
#[tauri::command]
#[specta::specta]
pub async fn open_fix_editor(app: AppHandle) -> Result<(), String> {
    open_fix_editor_window(&app)
}

#[cfg(test)]
mod tests {
    use super::choose_fix_transcript;

    #[test]
    fn fix_transcript_prefers_last_pasted_text_over_history() {
        let transcript = choose_fix_transcript(
            Some("fresh pasted transcript".to_string()),
            Some("stale history transcript".to_string()),
        );

        assert_eq!(transcript.as_deref(), Some("fresh pasted transcript"));
    }

    #[test]
    fn fix_transcript_falls_back_to_history_when_pasted_text_is_blank() {
        let transcript = choose_fix_transcript(
            Some("   ".to_string()),
            Some("history transcript".to_string()),
        );

        assert_eq!(transcript.as_deref(), Some("history transcript"));
    }

    #[test]
    fn fix_transcript_returns_none_when_sources_are_blank_or_missing() {
        let transcript = choose_fix_transcript(Some("   ".to_string()), None);

        assert_eq!(transcript, None);
    }
}
