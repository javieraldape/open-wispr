//! Corrections manager — owns the SQLite store and a cached, validated
//! `CorrectionSet`, and is the ONLY component the pipeline talks to.
//!
//!   mutations                         reads (hot path)
//!   ─────────                         ────────────────
//!   add_learned / add_manual ─┐       apply(text)
//!   delete ───────────────────┤         │ RwLock read → Arc<CorrectionSet>
//!                             ▼         │ catch_unwind(set.apply)
//!                     store (Mutex)     │   panic ⇒ log + uncorrected text
//!                             │         │ touch_applied(fired keys)
//!                    rebuild cache ◀────┘
//!                    (RwLock write)
//!
//! The cached set is rebuilt on every mutation; `apply` never touches the
//! store except to bump LRU recency. A store that failed to open degrades to
//! a disabled manager (empty set, mutations rejected) — a broken corrections
//! DB must never block dictation (see the plan's error registry).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use chrono::Utc;
use log::{error, warn};
use tauri::AppHandle;

use crate::corrections::{
    tokenizer, ApplyOutcome, CorrectionSet, CorrectionSource, CorrectionsStore, RejectedPair,
    StoreError, StoredCorrection,
};

/// The corrections database file, next to Handy's `history.db` (separate
/// file — see the rationale in `corrections::store`).
const DB_FILE_NAME: &str = "corrections.sqlite";

/// The built set plus the pairs the build rejected (surfaced to the E6 panel
/// so a stored-but-inactive pair is never silently invisible).
#[derive(Default)]
struct CachedBuild {
    set: Arc<CorrectionSet>,
    rejected: Vec<RejectedPair>,
}

/// Result of `learn_or_reverse`: whether the edit taught a new pair or undid
/// an existing (inverse) one. The command layer reports the distinction to the
/// user ("learned X → Y" vs "unlearned Y → X").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReversalOutcome {
    /// A new pair was learned; carries its `heard_key`.
    Learned(String),
    /// An existing inverse pair was found and DELETED (ping-pong prevention);
    /// carries the deleted pair's `heard_key`.
    Reversed(String),
}

pub struct CorrectionsManager {
    /// `None` when the store failed to open (disabled mode).
    store: Mutex<Option<CorrectionsStore>>,
    cache: RwLock<CachedBuild>,
    /// `heard_key`s that fired on the MOST RECENT `apply()` call — a single
    /// slot, overwritten every call. This is what "the pairs applied to this
    /// transcript" means for the E1 reversal rule (`learn_or_reverse`): the
    /// pipeline only ever fixes the single latest transcript, so scoping
    /// reversal to the last apply's fired keys (rather than scanning every
    /// stored pair for a global inverse) matches the spec without a schema
    /// change to persist per-transcript applied-correction records.
    last_applied_keys: Mutex<Vec<String>>,
    /// Test hook: force the apply pass to panic to prove the catch_unwind
    /// degradation path. Always present (one relaxed load per apply);
    /// settable only from tests.
    #[allow(dead_code)] // read by apply(), which T4 wires into the emit paths
    force_panic: std::sync::atomic::AtomicBool,
}

impl CorrectionsManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
        std::fs::create_dir_all(&app_data_dir)?;
        let store = CorrectionsStore::open(&app_data_dir.join(DB_FILE_NAME))?;
        Ok(Self::from_store(store))
    }

    /// Build a manager around an already-open store (no Tauri context —
    /// used by tests and available to future headless callers).
    pub fn from_store(store: CorrectionsStore) -> Self {
        let manager = Self {
            store: Mutex::new(Some(store)),
            cache: RwLock::new(CachedBuild::default()),
            last_applied_keys: Mutex::new(Vec::new()),
            force_panic: std::sync::atomic::AtomicBool::new(false),
        };
        if let Err(e) = manager.rebuild() {
            // Degraded but alive: an unreadable store yields an empty set.
            error!("Failed to build correction set from store: {e}");
        }
        manager
    }

    /// A manager with no store: applies nothing, rejects mutations. Used
    /// when the corrections DB cannot be opened so dictation still works.
    pub fn disabled() -> Self {
        Self {
            store: Mutex::new(None),
            cache: RwLock::new(CachedBuild::default()),
            last_applied_keys: Mutex::new(Vec::new()),
            force_panic: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Apply the cached correction set to `text`.
    ///
    /// Panic-safe: a panic inside the apply pass is caught, logged, and the
    /// UNCORRECTED text is returned — a lost correction must never lose a
    /// dictation. Fired pairs get their LRU recency bumped (best-effort).
    pub fn apply(&self, text: &str) -> ApplyOutcome {
        let set = Arc::clone(&self.cache.read().unwrap_or_else(|p| p.into_inner()).set);

        let force_panic = self.force_panic.load(std::sync::atomic::Ordering::Relaxed);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            if force_panic {
                panic!("forced corrections panic (test hook)");
            }
            set.apply(text)
        }));

        match outcome {
            Ok(outcome) => {
                // Record which keys fired on THIS call, overwriting whatever
                // the previous apply recorded — a single slot for "the pairs
                // applied to the last transcript" (see the field doc on
                // `last_applied_keys`). Recorded even when empty, so a
                // transcript with no corrections correctly clears stale state
                // from a prior one.
                let keys: Vec<String> = outcome
                    .applied
                    .iter()
                    .map(|a| a.heard_key.clone())
                    .collect();
                *self
                    .last_applied_keys
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = keys.clone();

                if !keys.is_empty() {
                    if let Some(store) = self
                        .store
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .as_ref()
                    {
                        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
                        if let Err(e) = store.touch_applied(&key_refs, Utc::now()) {
                            warn!("Failed to record correction usage: {e}");
                        }
                    }
                }
                outcome
            }
            Err(_) => {
                error!("Corrections apply pass panicked; emitting uncorrected text");
                ApplyOutcome {
                    text: text.to_string(),
                    applied: Vec::new(),
                }
            }
        }
    }

    /// Store a pair learned from the E1 fix flow. Learned pairs are
    /// verbatim (eval spike finding F4). Returns the pair's heard_key.
    #[allow(dead_code)] // consumed by the T5 E1 fix flow
    pub fn add_learned(&self, heard: &str, correct: &str) -> Result<String, StoreError> {
        self.mutate(|store| {
            store.upsert(heard, correct, CorrectionSource::Learned, true, Utc::now())
        })
    }

    /// Learn a pair from the E1 fix flow, OR reverse a prior learning.
    ///
    /// REVERSAL RULE (ping-pong prevention): if a pair that was ACTUALLY
    /// APPLIED TO THIS TRANSCRIPT (i.e. its `heard_key` is in
    /// `last_applied_keys` — the fired keys from the most recent `apply()`
    /// call) is the exact inverse of what we're about to learn — i.e. a
    /// stored pair whose `correct_text` normalizes to this `heard` AND whose
    /// `heard_text` normalizes to this `correct` — we DELETE that stored pair
    /// instead of learning the inverse. A user who fixed A→B and later
    /// "fixes" B back to A wants the rule forgotten, not its mathematical
    /// inverse learned (which would fight the original on every dictation).
    ///
    /// Scoped to `last_applied_keys` (not a global scan of every stored pair)
    /// so an unrelated pair that happens to be a mathematical inverse — but
    /// never fired on this transcript — is never deleted by surprise.
    ///
    /// Otherwise, learns the pair via `add_learned` (verbatim, as usual).
    #[allow(dead_code)] // consumed by the T5 E1 fix command layer
    pub fn learn_or_reverse(
        &self,
        heard: &str,
        correct: &str,
    ) -> Result<ReversalOutcome, StoreError> {
        // Normalized key form the store already uses for heard_key.
        let heard_key = tokenizer::normalized_tokens(heard).join(" ");
        let correct_key = tokenizer::normalized_tokens(correct).join(" ");

        let applied_keys = self
            .last_applied_keys
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();

        // Scan ONLY pairs that fired on this transcript for the exact
        // inverse: stored.heard == our correct AND stored.correct == our
        // heard (both compared in normalized form).
        let existing = self.list()?;
        let inverse = existing.into_iter().find(|row| {
            if !applied_keys.contains(&row.heard_key) {
                return false;
            }
            let stored_heard_key = tokenizer::normalized_tokens(&row.heard_text).join(" ");
            let stored_correct_key = tokenizer::normalized_tokens(&row.correct_text).join(" ");
            stored_heard_key == correct_key && stored_correct_key == heard_key
        });

        if let Some(row) = inverse {
            self.delete(&row.heard_key)?;
            return Ok(ReversalOutcome::Reversed(row.heard_key));
        }

        let key = self.add_learned(heard, correct)?;
        Ok(ReversalOutcome::Learned(key))
    }

    /// Store a manually entered pair with an explicit casing mode.
    #[allow(dead_code)] // consumed by the T6 E6 panel (manual vocabulary)
    pub fn add_manual(
        &self,
        heard: &str,
        correct: &str,
        verbatim: bool,
    ) -> Result<String, StoreError> {
        self.mutate(|store| {
            store.upsert(
                heard,
                correct,
                CorrectionSource::Manual,
                verbatim,
                Utc::now(),
            )
        })
    }

    /// Delete a pair by heard_key. Returns whether it existed.
    #[allow(dead_code)] // consumed by T5 (reversal rule) and the T6 E6 panel
    pub fn delete(&self, heard_key: &str) -> Result<bool, StoreError> {
        self.mutate(|store| store.delete(heard_key))
    }

    /// All stored rows, newest first (the future E6 panel's list).
    #[allow(dead_code)] // consumed by the T6 E6 panel
    pub fn list(&self) -> Result<Vec<StoredCorrection>, StoreError> {
        match self
            .store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            Some(store) => store.list(),
            None => Err(StoreError::Unavailable),
        }
    }

    /// Pairs the last build rejected, with their reasons (E6 panel).
    #[allow(dead_code)] // consumed by the T6 E6 panel
    pub fn rejected(&self) -> Vec<RejectedPair> {
        self.cache
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .rejected
            .clone()
    }

    /// Run a store mutation, then rebuild the cached set.
    fn mutate<T>(
        &self,
        op: impl FnOnce(&CorrectionsStore) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let result = {
            let guard = self.store.lock().unwrap_or_else(|p| p.into_inner());
            match guard.as_ref() {
                Some(store) => op(store)?,
                None => return Err(StoreError::Unavailable),
            }
        };
        self.rebuild()?;
        Ok(result)
    }

    /// Rebuild the cached `CorrectionSet` from the store.
    fn rebuild(&self) -> Result<(), StoreError> {
        let pairs = {
            let guard = self.store.lock().unwrap_or_else(|p| p.into_inner());
            match guard.as_ref() {
                Some(store) => store.all_pairs()?,
                None => Vec::new(),
            }
        };
        let build = CorrectionSet::build(&pairs);
        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        cache.set = Arc::new(build.set);
        cache.rejected = build.rejected;
        Ok(())
    }

    #[cfg(test)]
    fn set_force_panic(&self, on: bool) {
        self.force_panic
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }
}

/// THE CHOKE POINT (T4): corrections applied as the FINAL text transformation
/// before emit. Every text-producing path in this fork — batch transcription
/// finalize, streaming finalize, the optional LLM post-process, and the
/// `--transcribe-file` CLI path — must route its final text through this
/// function, in that order (corrections run AFTER the LLM post-process when
/// it's enabled; corrections are always last). See the plan's "Pipeline
/// Contract": one parity test locks UI/CLI identity by asserting both call
/// sites use this exact function.
///
/// Deliberately a plain `&CorrectionsManager` (not `&AppHandle`) so it is
/// directly unit-testable with a temp-dir store and so the CLI headless path
/// (which builds its own manager instance, no full app setup) can call it
/// identically to the UI path.
pub fn apply_corrections(manager: &CorrectionsManager, text: &str) -> String {
    manager.apply(text).text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corrections::PairError;

    fn temp_manager() -> (tempfile::TempDir, CorrectionsManager) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            CorrectionsStore::open(&dir.path().join("corrections.sqlite")).expect("open store");
        (dir, CorrectionsManager::from_store(store))
    }

    #[test]
    fn add_learned_applies_verbatim_on_next_dictation() {
        let (_dir, manager) = temp_manager();
        manager.add_learned("Devkit", "devkit").expect("add");
        // Learned pairs are verbatim: casing is forced, not adapted.
        assert_eq!(manager.apply("deploy Devkit now").text, "deploy devkit now");
        assert_eq!(manager.apply("DEVKIT!").text, "devkit!");
    }

    #[test]
    fn add_manual_respects_verbatim_flag() {
        let (_dir, manager) = temp_manager();
        manager
            .add_manual("pushear", "push", false)
            .expect("adaptive");
        assert_eq!(manager.apply("Pushear el branch").text, "Push el branch");
        manager
            .add_manual("Devkit", "devkit", true)
            .expect("verbatim");
        assert_eq!(manager.apply("Devkit YA").text, "devkit YA");
    }

    #[test]
    fn invalid_pair_is_rejected_with_typed_error() {
        let (_dir, manager) = temp_manager();
        match manager.add_learned("same", "same") {
            Err(StoreError::InvalidPair(PairError::Identical)) => {}
            other => panic!("expected Identical rejection, got {other:?}"),
        }
    }

    #[test]
    fn learn_or_reverse_deletes_the_inverse_instead_of_learning_it() {
        let (_dir, manager) = temp_manager();
        // Teach "when you hear Acme, write acme".
        manager.add_learned("Acme", "acme").expect("add");
        assert_eq!(manager.apply("deploy Acme now").text, "deploy acme now");

        // Now "fix" the exact inverse (heard=acme, correct=Acme). This must
        // DELETE the stored pair, not learn acme→Acme.
        let outcome = manager.learn_or_reverse("acme", "Acme").expect("reverse");
        assert!(matches!(outcome, ReversalOutcome::Reversed(_)));

        // The store is empty and NEITHER direction transforms anymore.
        assert!(manager.list().expect("list").is_empty());
        assert_eq!(manager.apply("deploy Acme now").text, "deploy Acme now");
        assert_eq!(manager.apply("deploy acme now").text, "deploy acme now");
    }

    #[test]
    fn learn_or_reverse_learns_a_fresh_pair() {
        let (_dir, manager) = temp_manager();
        let outcome = manager
            .learn_or_reverse("cascad", "Cascade")
            .expect("learn");
        assert!(matches!(outcome, ReversalOutcome::Learned(_)));
        assert_eq!(manager.apply("go to cascad").text, "go to Cascade");
    }

    #[test]
    fn learn_or_reverse_ignores_a_global_inverse_that_did_not_fire_on_this_transcript() {
        // A stored pair ("Acme" -> "acme") is the mathematical inverse of
        // the fix we're about to submit ("acme" -> "Acme"), but it never
        // applied to THIS transcript (we call apply() on unrelated text right
        // before the fix). The reversal rule must be scoped to
        // `last_applied_keys`, not a global scan of every stored pair — so
        // this must LEARN acme->Acme as a fresh pair, not delete the
        // unrelated stored one.
        let (_dir, manager) = temp_manager();
        manager.add_learned("Acme", "acme").expect("add");

        // Apply to text that does NOT contain "Acme" at all, so nothing
        // fires and last_applied_keys is empty for this "transcript".
        let outcome = manager.apply("nothing to see here");
        assert!(outcome.applied.is_empty());

        let outcome = manager
            .learn_or_reverse("acme", "Acme")
            .expect("learn, not reverse");
        assert!(
            matches!(outcome, ReversalOutcome::Learned(_)),
            "expected Learned since the inverse never fired on this transcript, got {outcome:?}"
        );

        // The unrelated one was NOT deleted. Because this specific inverse
        // has the same normalized heard key ("Acme" and "acme" normalize
        // identically), the store's last-write-wins upsert replaces it.
        let rows = manager.list().expect("list");
        assert_eq!(rows.len(), 1, "the row should be replaced, got {rows:?}");
        assert_eq!(rows[0].heard_text, "acme");
        assert_eq!(rows[0].correct_text, "Acme");
    }

    #[test]
    fn delete_stops_application() {
        let (_dir, manager) = temp_manager();
        let key = manager.add_learned("cascad", "Cascade").expect("add");
        assert_eq!(manager.apply("go to cascad").text, "go to Cascade");
        assert!(manager.delete(&key).expect("delete"));
        assert_eq!(manager.apply("go to cascad").text, "go to cascad");
    }

    #[test]
    fn build_rejections_are_surfaced_with_reasons() {
        let (_dir, manager) = temp_manager();
        // Passes per-pair validation but fails the build's fixed-point
        // check: "beta" → "beta max" re-triggers on its own output.
        manager
            .add_manual("beta", "beta max", true)
            .expect("insert ok");
        let rejected = manager.rejected();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].error, PairError::NotIdempotent);
        // And it must not fire.
        assert_eq!(manager.apply("beta test").text, "beta test");
    }

    #[test]
    fn apply_bumps_last_applied_for_fired_pairs_only() {
        let (_dir, manager) = temp_manager();
        manager.add_learned("cascad", "Cascade").expect("add");
        manager.add_learned("acme", "Acme").expect("add");
        manager.apply("push to cascad");
        let rows = manager.list().expect("list");
        let by_key = |k: &str| rows.iter().find(|r| r.heard_key == k).unwrap().clone();
        assert!(by_key("cascad").last_applied_at.is_some());
        assert!(by_key("acme").last_applied_at.is_none());
    }

    #[test]
    fn panic_in_apply_degrades_to_uncorrected_text() {
        let (_dir, manager) = temp_manager();
        manager.add_learned("cascad", "Cascade").expect("add");
        manager.set_force_panic(true);
        let outcome = manager.apply("go to cascad");
        assert_eq!(outcome.text, "go to cascad");
        assert!(outcome.applied.is_empty());
        // And recovers once the panic source is gone.
        manager.set_force_panic(false);
        assert_eq!(manager.apply("go to cascad").text, "go to Cascade");
    }

    #[test]
    fn disabled_manager_is_identity_and_rejects_mutations() {
        let manager = CorrectionsManager::disabled();
        assert_eq!(manager.apply("hola mundo").text, "hola mundo");
        assert!(matches!(
            manager.add_learned("a", "b"),
            Err(StoreError::Unavailable)
        ));
        assert!(matches!(manager.list(), Err(StoreError::Unavailable)));
    }

    #[test]
    fn persists_across_manager_restarts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("corrections.sqlite");
        {
            let manager =
                CorrectionsManager::from_store(CorrectionsStore::open(&path).expect("open"));
            manager.add_learned("open whisper", "OpenWhisper").unwrap();
        }
        let manager =
            CorrectionsManager::from_store(CorrectionsStore::open(&path).expect("reopen"));
        assert_eq!(
            manager.apply("el proyecto open whisper").text,
            "el proyecto OpenWhisper"
        );
    }

    /// T4 choke point, part (a): `apply_corrections` is a pure passthrough to
    /// `manager.apply(text).text` — the two UI/CLI call sites (actions.rs and
    /// lib.rs's `--transcribe-file` path) both call this exact function, so
    /// proving they compute the same corrected text for the same manager
    /// reduces to this one direct equivalence.
    #[test]
    fn apply_corrections_matches_manager_apply_text() {
        let (_dir, manager) = temp_manager();
        manager.add_learned("cascad", "Cascade").expect("add");
        let direct = manager.apply("go to cascad").text;
        let via_choke_point = apply_corrections(&manager, "go to cascad");
        assert_eq!(direct, via_choke_point);
        assert_eq!(via_choke_point, "go to Cascade");
    }

    /// T4 choke point, part (b): integration-style — temp-dir store, a
    /// learned pair added through the manager, run through the choke point,
    /// correction applied AND `last_applied_at` bumped (proves the choke
    /// point exercises the real LRU-recency side effect, not a shortcut).
    #[test]
    fn apply_corrections_applies_pair_and_bumps_recency() {
        let (_dir, manager) = temp_manager();
        let key = manager.add_learned("cascad", "Cascade").expect("add");
        let before = manager
            .list()
            .expect("list")
            .into_iter()
            .find(|r| r.heard_key == key)
            .expect("row exists");
        assert!(before.last_applied_at.is_none());

        let corrected = apply_corrections(&manager, "push to cascad now");
        assert_eq!(corrected, "push to Cascade now");

        let after = manager
            .list()
            .expect("list")
            .into_iter()
            .find(|r| r.heard_key == key)
            .expect("row exists");
        assert!(after.last_applied_at.is_some());
    }

    /// T4 choke point, part (c): the forced-panic hook, exercised through the
    /// choke point itself (not just `manager.apply` directly) — a panic in
    /// the apply pass must degrade to uncorrected text at the seam every
    /// caller actually uses.
    #[test]
    fn apply_corrections_degrades_to_uncorrected_text_on_panic() {
        let (_dir, manager) = temp_manager();
        manager.add_learned("cascad", "Cascade").expect("add");
        manager.set_force_panic(true);
        assert_eq!(
            apply_corrections(&manager, "go to cascad"),
            "go to cascad"
        );
        manager.set_force_panic(false);
        assert_eq!(
            apply_corrections(&manager, "go to cascad"),
            "go to Cascade"
        );
    }
}
