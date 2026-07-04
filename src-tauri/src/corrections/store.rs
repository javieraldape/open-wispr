//! SQLite persistence for correction pairs.
//!
//! Pure rusqlite — NO Tauri dependencies. The store lives in its OWN database
//! file (`corrections.sqlite`, next to Handy's `history.db`) rather than
//! inside `history.db`: rusqlite_migration tracks one `user_version` per file,
//! so sharing history.db would force this table into `HistoryManager`'s
//! migration list — coupling the pure store to a Tauri-bound manager and
//! creating merge friction with upstream Handy's history migrations. A
//! separate file keeps the store self-contained, openable by the eval CLI
//! (a second process — hence WAL), and trivially unit-testable.
//!
//!   write path                              read path
//!   ──────────                              ─────────
//!   NFC-normalize heard/correct             all_pairs() ORDER BY created_at
//!     → validate_pair (shared with            → CorrectionSet::build
//!       CorrectionSet::build)                   (last-write-wins matches
//!     → heard_key = normalized tokens           insertion recency)
//!     → upsert (heard_key UNIQUE,
//!       last-write-wins)
//!     → LRU-evict over the cap
//!       (oldest last_applied_at, NULLs
//!       first — never the fresh row)

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection};
use rusqlite_migration::{Migrations, M};
use unicode_normalization::UnicodeNormalization;

use super::apply::{validate_pair, CorrectionPair, PairError};

/// Growth cap: LRU eviction keeps at most this many pairs (reviewed plan).
pub const MAX_STORED_PAIRS: usize = 2000;

static MIGRATIONS: &[M] = &[M::up(
    "CREATE TABLE IF NOT EXISTS corrections (
        id INTEGER PRIMARY KEY,
        heard_key TEXT NOT NULL UNIQUE,
        heard_text TEXT NOT NULL,
        correct_text TEXT NOT NULL,
        source TEXT NOT NULL CHECK(source IN ('learned','manual')),
        verbatim INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        last_applied_at TEXT
    );",
)];

/// Where a pair came from: taught by the E1 fix flow, or entered manually.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionSource {
    Learned,
    Manual,
}

impl CorrectionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            CorrectionSource::Learned => "learned",
            CorrectionSource::Manual => "manual",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "learned" => Some(CorrectionSource::Learned),
            "manual" => Some(CorrectionSource::Manual),
            _ => None,
        }
    }
}

/// Typed store errors. `InvalidPair` carries the same per-pair rejection
/// reasons `CorrectionSet::build` reports, so callers (E6 panel) can show
/// one consistent message per cause.
#[derive(Debug)]
pub enum StoreError {
    InvalidPair(PairError),
    Db(rusqlite::Error),
    Migration(rusqlite_migration::Error),
    /// The store could not be opened at startup; corrections are disabled
    /// but dictation continues (see the error registry in the plan).
    Unavailable,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::InvalidPair(e) => write!(f, "invalid correction: {e}"),
            StoreError::Db(e) => write!(f, "corrections database error: {e}"),
            StoreError::Migration(e) => write!(f, "corrections migration error: {e}"),
            StoreError::Unavailable => write!(f, "corrections store is unavailable"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e)
    }
}

impl From<rusqlite_migration::Error> for StoreError {
    fn from(e: rusqlite_migration::Error) -> Self {
        StoreError::Migration(e)
    }
}

/// A full stored row, for the E6 "words it learned" panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCorrection {
    pub id: i64,
    pub heard_key: String,
    pub heard_text: String,
    pub correct_text: String,
    pub source: CorrectionSource,
    pub verbatim: bool,
    pub created_at: String,
    pub last_applied_at: Option<String>,
}

pub struct CorrectionsStore {
    conn: Connection,
    cap: usize,
}

impl CorrectionsStore {
    /// Open (or create) the store at `path`, enabling WAL — the eval CLI runs
    /// as a second process against the same file — and running migrations.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    fn init(mut conn: Connection) -> Result<Self, StoreError> {
        // journal_mode returns the resulting mode as a row, so use query_row
        // (pragma_update rejects statements that return rows). WAL is
        // persistent per database file; in-memory DBs report "memory".
        let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        migrations.to_latest(&mut conn)?;
        Ok(Self {
            conn,
            cap: MAX_STORED_PAIRS,
        })
    }

    /// Insert or replace (last-write-wins on `heard_key`) a correction pair.
    ///
    /// `heard`/`correct` are NFC-normalized at write, then validated with the
    /// same per-pair checks `CorrectionSet::build` applies. Returns the
    /// pair's `heard_key`. Evicts over-cap rows (oldest `last_applied_at`
    /// first, NULLs before any value; the fresh row is never evicted).
    pub fn upsert(
        &self,
        heard: &str,
        correct: &str,
        source: CorrectionSource,
        verbatim: bool,
        now: DateTime<Utc>,
    ) -> Result<String, StoreError> {
        let heard_text: String = heard.nfc().collect();
        let correct_text: String = correct.nfc().collect();
        let candidate = CorrectionPair {
            heard: heard_text.clone(),
            correct: correct_text.clone(),
            verbatim,
        };
        let heard_tokens = validate_pair(&candidate).map_err(StoreError::InvalidPair)?;
        let heard_key = heard_tokens.join(" ");
        let created_at = format_ts(now);

        // Last-write-wins: replace content and refresh created_at so build's
        // insertion-order recency matches the newest teach. last_applied_at
        // is intentionally kept — re-teaching does not reset LRU standing.
        self.conn.execute(
            "INSERT INTO corrections (heard_key, heard_text, correct_text, source, verbatim, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(heard_key) DO UPDATE SET
                heard_text = excluded.heard_text,
                correct_text = excluded.correct_text,
                source = excluded.source,
                verbatim = excluded.verbatim,
                created_at = excluded.created_at",
            params![
                heard_key,
                heard_text,
                correct_text,
                source.as_str(),
                verbatim,
                created_at,
            ],
        )?;

        self.evict_over_cap(&heard_key)?;
        Ok(heard_key)
    }

    /// LRU eviction: delete the excess rows with the oldest `last_applied_at`
    /// (SQLite sorts NULLs first under ASC, so never-applied rows go first),
    /// tie-broken by oldest `created_at` then lowest id. The row just
    /// inserted is excluded explicitly: it also has a NULL `last_applied_at`,
    /// and a full store of applied rows must never swallow a fresh teach.
    fn evict_over_cap(&self, protect_key: &str) -> Result<(), StoreError> {
        let count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM corrections", [], |row| row.get(0))?;
        if count <= self.cap {
            return Ok(());
        }
        let excess = (count - self.cap) as i64;
        self.conn.execute(
            "DELETE FROM corrections WHERE id IN (
                SELECT id FROM corrections
                WHERE heard_key != ?2
                ORDER BY last_applied_at ASC, created_at ASC, id ASC
                LIMIT ?1
            )",
            params![excess, protect_key],
        )?;
        Ok(())
    }

    /// Record that these pairs just fired, for LRU recency.
    pub fn touch_applied<S: AsRef<str>>(
        &self,
        heard_keys: &[S],
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let ts = format_ts(now);
        let mut stmt = self
            .conn
            .prepare("UPDATE corrections SET last_applied_at = ?1 WHERE heard_key = ?2")?;
        for key in heard_keys {
            stmt.execute(params![ts, key.as_ref()])?;
        }
        Ok(())
    }

    /// All pairs in `created_at` order (oldest first), the order
    /// `CorrectionSet::build` expects so its last-write-wins conflict rule
    /// matches insertion recency.
    pub fn all_pairs(&self) -> Result<Vec<CorrectionPair>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT heard_text, correct_text, verbatim FROM corrections
             ORDER BY created_at ASC, id ASC",
        )?;
        let pairs = stmt
            .query_map([], |row| {
                Ok(CorrectionPair {
                    heard: row.get(0)?,
                    correct: row.get(1)?,
                    verbatim: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(pairs)
    }

    /// Full rows, newest first (E6 panel order).
    pub fn list(&self) -> Result<Vec<StoredCorrection>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, heard_key, heard_text, correct_text, source, verbatim,
                    created_at, last_applied_at
             FROM corrections
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let source_raw: String = row.get(4)?;
                Ok(StoredCorrection {
                    id: row.get(0)?,
                    heard_key: row.get(1)?,
                    heard_text: row.get(2)?,
                    correct_text: row.get(3)?,
                    // The CHECK constraint guarantees a known value; default
                    // defensively rather than fail the whole listing.
                    source: CorrectionSource::parse(&source_raw)
                        .unwrap_or(CorrectionSource::Manual),
                    verbatim: row.get(5)?,
                    created_at: row.get(6)?,
                    last_applied_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete by heard_key. Returns whether a row existed.
    pub fn delete(&self, heard_key: &str) -> Result<bool, StoreError> {
        let n = self.conn.execute(
            "DELETE FROM corrections WHERE heard_key = ?1",
            params![heard_key],
        )?;
        Ok(n > 0)
    }

    pub fn len(&self) -> Result<usize, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM corrections", [], |row| row.get(0))?)
    }

    pub fn is_empty(&self) -> Result<bool, StoreError> {
        Ok(self.len()? == 0)
    }

    /// Test hook: shrink the LRU cap so eviction is exercised cheaply.
    #[cfg(test)]
    fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
    }
}

/// Fixed-width RFC3339 UTC ("Z") with millisecond precision: lexicographic
/// order equals chronological order, which the ORDER BY clauses rely on.
fn format_ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corrections::CorrectionSet;
    use chrono::TimeZone;

    fn open_temp() -> (tempfile::TempDir, CorrectionsStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CorrectionsStore::open(&dir.path().join("corrections.sqlite")).expect("open");
        (dir, store)
    }

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_750_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn insert_and_read_back_roundtrip() {
        let (_dir, store) = open_temp();
        store
            .upsert("cascad", "Cascade", CorrectionSource::Learned, true, ts(0))
            .expect("insert");
        let pairs = store.all_pairs().expect("all_pairs");
        assert_eq!(
            pairs,
            vec![CorrectionPair {
                heard: "cascad".into(),
                correct: "Cascade".into(),
                verbatim: true,
            }]
        );
    }

    #[test]
    fn upsert_same_heard_key_is_last_write_wins() {
        let (_dir, store) = open_temp();
        store
            .upsert("acme", "Acmy", CorrectionSource::Learned, true, ts(0))
            .expect("first");
        // Different casing/punctuation, same normalized key.
        let key = store
            .upsert("ACME", "Acme", CorrectionSource::Manual, false, ts(1))
            .expect("second");
        assert_eq!(key, "acme");
        assert_eq!(store.len().unwrap(), 1);
        let rows = store.list().expect("list");
        assert_eq!(rows[0].heard_text, "ACME");
        assert_eq!(rows[0].correct_text, "Acme");
        assert_eq!(rows[0].source, CorrectionSource::Manual);
        assert!(!rows[0].verbatim);
    }

    #[test]
    fn validation_rejects_bad_pairs_with_typed_errors() {
        let (_dir, store) = open_temp();
        let cases: Vec<(&str, &str, PairError)> = vec![
            ("", "x", PairError::EmptyHeard),
            (" ¿? ", "x", PairError::EmptyHeard),
            ("foo", "  ", PairError::EmptyCorrect),
            ("same", "same", PairError::Identical),
            ("a b c d e f g h i", "x", PairError::TooManyTokens),
        ];
        for (heard, correct, expected) in cases {
            match store.upsert(heard, correct, CorrectionSource::Manual, true, ts(0)) {
                Err(StoreError::InvalidPair(e)) => assert_eq!(e, expected),
                other => panic!("expected InvalidPair({expected:?}), got {other:?}"),
            }
        }
        assert!(store.is_empty().unwrap());
    }

    fn keys(store: &CorrectionsStore) -> Vec<String> {
        let mut k: Vec<String> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|r| r.heard_key)
            .collect();
        k.sort();
        k
    }

    #[test]
    fn lru_evicts_never_applied_rows_first() {
        let (_dir, mut store) = open_temp();
        store.set_cap(3);
        store
            .upsert("aaa", "A1", CorrectionSource::Learned, true, ts(0))
            .unwrap();
        store
            .upsert("bbb", "B1", CorrectionSource::Learned, true, ts(1))
            .unwrap();
        store
            .upsert("ccc", "C1", CorrectionSource::Learned, true, ts(2))
            .unwrap();
        // aaa and ccc have fired; bbb never applied → bbb (NULL) evicts first
        // even though aaa is older by created_at.
        store.touch_applied(&["aaa", "ccc"], ts(10)).unwrap();
        store
            .upsert("ddd", "D1", CorrectionSource::Learned, true, ts(3))
            .unwrap();
        assert_eq!(keys(&store), vec!["aaa", "ccc", "ddd"]);
    }

    #[test]
    fn lru_evicts_oldest_applied_and_protects_fresh_insert() {
        let (_dir, mut store) = open_temp();
        store.set_cap(3);
        store
            .upsert("aaa", "A1", CorrectionSource::Learned, true, ts(0))
            .unwrap();
        store
            .upsert("bbb", "B1", CorrectionSource::Learned, true, ts(1))
            .unwrap();
        store
            .upsert("ccc", "C1", CorrectionSource::Learned, true, ts(2))
            .unwrap();
        store.touch_applied(&["aaa"], ts(30)).unwrap();
        store.touch_applied(&["bbb"], ts(10)).unwrap();
        store.touch_applied(&["ccc"], ts(20)).unwrap();
        // Every existing row has fired. The fresh insert (NULL
        // last_applied_at) must NOT evict itself; the oldest-applied row
        // (bbb, ts 10) goes instead.
        store
            .upsert("ddd", "D1", CorrectionSource::Learned, true, ts(3))
            .unwrap();
        assert_eq!(keys(&store), vec!["aaa", "ccc", "ddd"]);
    }

    #[test]
    fn touch_applied_sets_last_applied_at() {
        let (_dir, store) = open_temp();
        store
            .upsert("foo", "bar", CorrectionSource::Learned, true, ts(0))
            .unwrap();
        assert_eq!(store.list().unwrap()[0].last_applied_at, None);
        store.touch_applied(&["foo"], ts(5)).unwrap();
        let rows = store.list().unwrap();
        assert_eq!(
            rows[0].last_applied_at.as_deref(),
            Some(&format_ts(ts(5))[..])
        );
    }

    #[test]
    fn nfd_input_is_stored_nfc() {
        let (_dir, store) = open_temp();
        // "café" typed in NFD (e + combining acute) on both sides.
        store
            .upsert(
                "cafe\u{0301}",
                "un cafe\u{0301} grande",
                CorrectionSource::Manual,
                false,
                ts(0),
            )
            .expect("insert");
        let rows = store.list().unwrap();
        assert_eq!(rows[0].heard_text, "caf\u{00e9}");
        assert_eq!(rows[0].correct_text, "un caf\u{00e9} grande");
        assert_eq!(rows[0].heard_key, "caf\u{00e9}");
    }

    #[test]
    fn nfc_normalization_makes_identical_pairs_identical() {
        let (_dir, store) = open_temp();
        // NFD heard vs NFC correct: identical after NFC → rejected.
        match store.upsert(
            "cafe\u{0301}",
            "caf\u{00e9}",
            CorrectionSource::Manual,
            true,
            ts(0),
        ) {
            Err(StoreError::InvalidPair(PairError::Identical)) => {}
            other => panic!("expected Identical rejection, got {other:?}"),
        }
    }

    #[test]
    fn delete_removes_row_and_reports_existence() {
        let (_dir, store) = open_temp();
        store
            .upsert("foo", "bar", CorrectionSource::Learned, true, ts(0))
            .unwrap();
        assert!(store.delete("foo").unwrap());
        assert!(!store.delete("foo").unwrap());
        assert!(store.is_empty().unwrap());
    }

    #[test]
    fn reload_from_disk_yields_identical_correction_set_behavior() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("corrections.sqlite");
        let text = "El Devkit deploy: cascad y open whisper listos";

        let before = {
            let store = CorrectionsStore::open(&path).expect("open");
            store
                .upsert("cascad", "Cascade", CorrectionSource::Learned, true, ts(0))
                .unwrap();
            store
                .upsert(
                    "open whisper",
                    "OpenWhisper",
                    CorrectionSource::Learned,
                    true,
                    ts(1),
                )
                .unwrap();
            store
                .upsert("Devkit", "devkit", CorrectionSource::Manual, true, ts(2))
                .unwrap();
            let set = CorrectionSet::build(&store.all_pairs().unwrap()).set;
            set.apply(text).text
        }; // store dropped — connection closed

        let store = CorrectionsStore::open(&path).expect("reopen");
        let set = CorrectionSet::build(&store.all_pairs().unwrap()).set;
        let after = set.apply(text).text;
        assert_eq!(before, after);
        assert_eq!(after, "El devkit deploy: Cascade y OpenWhisper listos");
    }

    #[test]
    fn all_pairs_order_gives_last_write_wins_in_build() {
        let (_dir, store) = open_temp();
        store
            .upsert("acme", "Acmy", CorrectionSource::Learned, true, ts(0))
            .unwrap();
        store
            .upsert("acme", "Acme", CorrectionSource::Learned, true, ts(1))
            .unwrap();
        // heard_key is UNIQUE so there is exactly one row, holding the newest
        // correct — build sees no conflict and applies the latest teach.
        let set = CorrectionSet::build(&store.all_pairs().unwrap()).set;
        assert_eq!(set.apply("acme app").text, "Acme app");
    }

    #[test]
    fn wal_mode_is_enabled_for_cross_process_access() {
        let (_dir, store) = open_temp();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }
}
