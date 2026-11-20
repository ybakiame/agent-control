use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::model::{
    Conflict, EvidenceEntry, EvidenceKind, GENESIS_HASH, HandoffRecord, Project, ProvenanceRecord,
    QueryContext, Rule, RuleMatch, RuleSuggestion, Session, SessionStatus, SuggestionStatus,
    VerificationReport,
};
use crate::search::{SearchHit, SearchKind};

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Concurrent creators (FIX1) must wait for the write lock instead of failing instantly.
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                task TEXT NOT NULL,
                project_path TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                snapshot_json TEXT NOT NULL,
                handoffs_json TEXT NOT NULL DEFAULT '[]',
                assurance_json TEXT NOT NULL DEFAULT '[]',
                goal TEXT NOT NULL DEFAULT '',
                work_items_json TEXT NOT NULL DEFAULT '[]',
                harness_session_id TEXT,
                predecessor TEXT,
                title TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS evidence (
                session_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                kind TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                payload_hash TEXT NOT NULL,
                prev_hash TEXT NOT NULL,
                entry_hash TEXT NOT NULL,
                PRIMARY KEY (session_id, seq)
            );
            CREATE TABLE IF NOT EXISTS provenance (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                session_id TEXT,
                agent_id TEXT NOT NULL,
                context_json TEXT NOT NULL,
                trace_json TEXT NOT NULL,
                conflicts_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                description TEXT,
                created_at TEXT NOT NULL,
                last_opened_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rule_suggestions (
                id TEXT PRIMARY KEY,
                project_path TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'project',
                agent_id TEXT NOT NULL,
                rule_id TEXT NOT NULL,
                title TEXT NOT NULL,
                topic TEXT,
                content TEXT NOT NULL,
                kind TEXT NOT NULL,
                enforcement TEXT NOT NULL DEFAULT 'advisory',
                tags_json TEXT NOT NULL DEFAULT '[]',
                rationale TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                reviewed_at TEXT,
                review_note TEXT,
                approved_rule_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_evidence_session ON evidence(session_id, seq);
            CREATE INDEX IF NOT EXISTS idx_provenance_session ON provenance(session_id);
            CREATE INDEX IF NOT EXISTS idx_suggestions_project ON rule_suggestions(project_path, status);
            ",
        )?;
        // Databases created before user-scope suggestions lack the scope column; add it once.
        let has_scope = conn
            .prepare("SELECT scope FROM rule_suggestions LIMIT 0")
            .is_ok();
        if !has_scope {
            conn.execute_batch(
                "ALTER TABLE rule_suggestions ADD COLUMN scope TEXT NOT NULL DEFAULT 'project';",
            )?;
        }
        // Assurance tiers (per-constraint achieved assurance) and rule enforcement intent were added
        // later; backfill the columns once for older databases.
        let has_assurance = conn
            .prepare("SELECT assurance_json FROM sessions LIMIT 0")
            .is_ok();
        if !has_assurance {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN assurance_json TEXT NOT NULL DEFAULT '[]';",
            )?;
        }
        let has_enforcement = conn
            .prepare("SELECT enforcement FROM rule_suggestions LIMIT 0")
            .is_ok();
        if !has_enforcement {
            conn.execute_batch(
                "ALTER TABLE rule_suggestions ADD COLUMN enforcement TEXT NOT NULL DEFAULT 'advisory';",
            )?;
        }
        // Resumable work state (goal + done/doing/todo) carried across handoffs; backfill once.
        let has_work_state = conn.prepare("SELECT goal FROM sessions LIMIT 0").is_ok();
        if !has_work_state {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN goal TEXT NOT NULL DEFAULT '';
                 ALTER TABLE sessions ADD COLUMN work_items_json TEXT NOT NULL DEFAULT '[]';",
            )?;
        }
        // Harness-native conversation id (UX5); nullable, backfill once.
        let has_harness_sid = conn
            .prepare("SELECT harness_session_id FROM sessions LIMIT 0")
            .is_ok();
        if !has_harness_sid {
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN harness_session_id TEXT;")?;
        }
        // Predecessor link for same-task chain splits (UX1); nullable, backfill once.
        let has_predecessor = conn
            .prepare("SELECT predecessor FROM sessions LIMIT 0")
            .is_ok();
        if !has_predecessor {
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN predecessor TEXT;")?;
        }
        // Human-facing title (UX7); empty falls back to task at display time, no backfill.
        let has_title = conn.prepare("SELECT title FROM sessions LIMIT 0").is_ok();
        if !has_title {
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN title TEXT NOT NULL DEFAULT '';")?;
        }
        // Optimistic-concurrency counter for the work-state list (CONC1).
        let has_wsv = conn
            .prepare("SELECT work_state_version FROM sessions LIMIT 0")
            .is_ok();
        if !has_wsv {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN work_state_version INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        // Full-text shadow tables (P1). Rows store app-segmented tokens (CJK bigrams), so
        // unicode61 only splits on spaces; original titles stay in UNINDEXED display columns.
        // Rebuilt from source on every Store::open.
        let has_display_titles = conn
            .prepare("SELECT title_tokens FROM search_rules LIMIT 0")
            .is_ok();
        if !has_display_titles {
            conn.execute_batch(
                "DROP TABLE IF EXISTS search_rules;
                 DROP TABLE IF EXISTS search_sessions;
                 DROP TABLE IF EXISTS search_evidence;",
            )?;
        }
        conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS search_rules USING fts5(
                rule_id UNINDEXED, scope UNINDEXED, project_path UNINDEXED,
                title UNINDEXED, title_tokens, content,
                tokenize='unicode61'
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS search_sessions USING fts5(
                session_id UNINDEXED, project_path UNINDEXED, task UNINDEXED,
                task_tokens, goal_tokens, work_tokens,
                tokenize='unicode61'
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS search_evidence USING fts5(
                entry_id UNINDEXED, session_id UNINDEXED, project_path UNINDEXED,
                kind UNINDEXED, title UNINDEXED, payload_tokens,
                tokenize='unicode61'
            );
            ",
        )?;
        Ok(())
    }

    pub fn search_clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "DELETE FROM search_rules; DELETE FROM search_sessions; DELETE FROM search_evidence;",
        )?;
        Ok(())
    }

    pub fn search_sync_rule(&self, rule: &Rule, project_path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM search_rules WHERE rule_id = ?1", [&rule.id])?;
        conn.execute(
            "INSERT INTO search_rules (rule_id, scope, project_path, title, title_tokens, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                rule.id,
                serde_json::to_value(rule.scope)?
                    .as_str()
                    .unwrap_or_default(),
                project_path,
                rule.title,
                crate::search::index_text(&rule.title),
                crate::search::index_text(&rule.content),
            ],
        )?;
        Ok(())
    }

    pub fn search_remove_rule(&self, rule_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM search_rules WHERE rule_id = ?1", [rule_id])?;
        Ok(())
    }

    pub fn search_sync_session(&self, session: &Session) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        Self::search_sync_session_conn(&conn, session)
    }

    fn search_sync_session_conn(conn: &Connection, session: &Session) -> Result<()> {
        let id = session.id.to_string();
        conn.execute("DELETE FROM search_sessions WHERE session_id = ?1", [&id])?;
        let work = session
            .work_items
            .iter()
            .map(|w| w.title.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        conn.execute(
            "INSERT INTO search_sessions (session_id, project_path, task, task_tokens, goal_tokens, work_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                session.project_path.display().to_string(),
                session.task,
                crate::search::index_text(&session.task),
                crate::search::index_text(&session.goal),
                crate::search::index_text(&work),
            ],
        )?;
        Ok(())
    }

    pub fn search_sync_evidence(&self, entry: &EvidenceEntry, project_path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        Self::search_sync_evidence_conn(&conn, entry, project_path)
    }

    fn search_sync_evidence_conn(
        conn: &Connection,
        entry: &EvidenceEntry,
        project_path: &str,
    ) -> Result<()> {
        let id = entry.id.to_string();
        conn.execute("DELETE FROM search_evidence WHERE entry_id = ?1", [&id])?;
        let kind = serde_json::to_value(entry.kind)?
            .as_str()
            .unwrap_or_default()
            .to_string();
        conn.execute(
            "INSERT INTO search_evidence (entry_id, session_id, project_path, kind, title, payload_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                entry.session_id.to_string(),
                project_path,
                kind,
                format!("{kind} #{}", entry.seq),
                crate::search::index_text(&serde_json::to_string(&entry.payload)?),
            ],
        )?;
        Ok(())
    }

    pub fn search_query(
        &self,
        expr: &str,
        kind: Option<SearchKind>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let conn = self.conn.lock().unwrap();
        let kinds = match kind {
            Some(k) => vec![k],
            None => vec![
                SearchKind::Rules,
                SearchKind::Sessions,
                SearchKind::Evidence,
            ],
        };
        let mut hits = Vec::new();
        for k in kinds {
            let (table, id_col, title_col, snip_col) = match k {
                SearchKind::Rules => ("search_rules", "rule_id", "title", 5),
                SearchKind::Sessions => ("search_sessions", "session_id", "task", 3),
                SearchKind::Evidence => ("search_evidence", "entry_id", "title", 5),
            };
            let mut sql = format!(
                "SELECT {id_col}, {title_col}, snippet({table}, {snip_col}, '[', ']', '…', 10),
                        project_path, bm25({table})
                 FROM {table} WHERE {table} MATCH ?1"
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(expr.to_string())];
            if let Some(p) = project {
                params.push(Box::new(p.to_string()));
                sql.push_str(&format!(" AND project_path = ?{}", params.len()));
            }
            params.push(Box::new(limit as i64));
            sql.push_str(&format!(" ORDER BY bm25({table}) LIMIT ?{}", params.len()));
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                |row| {
                    Ok(SearchHit {
                        kind: k,
                        id: row.get(0)?,
                        title: row.get(1)?,
                        snippet: row.get(2)?,
                        project_path: row.get(3)?,
                        score: row.get(4)?,
                    })
                },
            )?;
            for row in rows {
                hits.push(row?);
            }
        }
        hits.sort_by(|a, b| a.score.total_cmp(&b.score));
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn upsert_session(&self, session: &Session) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        Self::upsert_session_conn(&conn, session)
    }

    /// Run `f` inside a `BEGIN IMMEDIATE` transaction so concurrent writers serialize: the
    /// get-or-create check and the insert happen under one write lock (FIX1 TOCTOU fix).
    pub fn with_immediate_txn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        match f(&conn) {
            Ok(v) => {
                conn.execute_batch("COMMIT;")?;
                Ok(v)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Backstop against duplicate active sessions per (project, agent, task): even a writer that
    /// bypasses the transaction cannot insert a second active row for the same triple. The status
    /// column stores the serde JSON string, hence the quoted predicate literal.
    pub fn ensure_active_triple_index(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_active_triple
             ON sessions(project_path, agent_id, task) WHERE status = '\"active\"';",
        )?;
        Ok(())
    }

    pub fn list_sessions_conn(conn: &Connection) -> Result<Vec<Session>> {
        let mut stmt = conn.prepare(
            "SELECT id, task, project_path, agent_id, status, created_at, updated_at, snapshot_json, handoffs_json, assurance_json, goal, work_items_json, harness_session_id, predecessor, title, work_state_version FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], session_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn upsert_session_conn(conn: &Connection, session: &Session) -> Result<()> {
        conn.execute(
            "INSERT INTO sessions (id, task, project_path, agent_id, status, created_at, updated_at, snapshot_json, handoffs_json, assurance_json, goal, work_items_json, harness_session_id, predecessor, title, work_state_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(id) DO UPDATE SET
                task = excluded.task,
                project_path = excluded.project_path,
                agent_id = excluded.agent_id,
                status = excluded.status,
                updated_at = excluded.updated_at,
                snapshot_json = excluded.snapshot_json,
                handoffs_json = excluded.handoffs_json,
                assurance_json = excluded.assurance_json,
                goal = excluded.goal,
                work_items_json = excluded.work_items_json,
                harness_session_id = excluded.harness_session_id,
                predecessor = excluded.predecessor,
                title = excluded.title,
                work_state_version = excluded.work_state_version",
            rusqlite::params![
                session.id.to_string(),
                session.task,
                session.project_path.to_string_lossy(),
                session.agent_id,
                serde_json::to_string(&session.status)?,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
                serde_json::to_string(&session.constraint_snapshot)?,
                serde_json::to_string(&session.handoffs)?,
                serde_json::to_string(&session.assurance)?,
                session.goal,
                serde_json::to_string(&session.work_items)?,
                session.harness_session_id,
                session.predecessor.map(|id| id.to_string()),
                session.title,
                session.work_state_version as i64,
            ],
        )?;
        Self::search_sync_session_conn(conn, session)?;
        Ok(())
    }

    pub fn get_session(&self, id: Uuid) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task, project_path, agent_id, status, created_at, updated_at, snapshot_json, handoffs_json, assurance_json, goal, work_items_json, harness_session_id, predecessor, title, work_state_version FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(session_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        Self::list_sessions_conn(&conn)
    }

    /// Remove a session together with its evidence chain and provenance record: they are
    /// session-scoped, so leaving them behind would orphan unverifiable rows.
    pub fn delete_session(&self, id: Uuid) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let snapshot_json: Option<String> = conn
            .query_row(
                "SELECT snapshot_json FROM sessions WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let provenance_id = snapshot_json
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .and_then(|v| {
                v.get("provenance_id")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
            });
        conn.execute(
            "DELETE FROM evidence WHERE session_id = ?1",
            [id.to_string()],
        )?;
        conn.execute(
            "DELETE FROM provenance WHERE session_id = ?1",
            [id.to_string()],
        )?;
        if let Some(pid) = provenance_id {
            conn.execute("DELETE FROM provenance WHERE id = ?1", [pid])?;
        }
        conn.execute("DELETE FROM sessions WHERE id = ?1", [id.to_string()])?;
        conn.execute(
            "DELETE FROM search_sessions WHERE session_id = ?1",
            [id.to_string()],
        )?;
        conn.execute(
            "DELETE FROM search_evidence WHERE session_id = ?1",
            [id.to_string()],
        )?;
        Ok(())
    }

    pub fn append_evidence(
        &self,
        session_id: Uuid,
        kind: EvidenceKind,
        agent_id: &str,
        payload: serde_json::Value,
    ) -> Result<EvidenceEntry> {
        let conn = self.conn.lock().unwrap();
        let (seq, prev_hash) = evidence_head(&conn, session_id)?;
        let entry = EvidenceEntry::seal(
            seq + 1,
            session_id,
            Utc::now(),
            kind,
            agent_id.to_string(),
            payload,
            prev_hash,
        );
        conn.execute(
            "INSERT INTO evidence (session_id, seq, id, timestamp, kind, agent_id, payload_json, payload_hash, prev_hash, entry_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                entry.session_id.to_string(),
                entry.seq as i64,
                entry.id.to_string(),
                entry.timestamp.to_rfc3339(),
                serde_json::to_string(&entry.kind)?,
                entry.agent_id,
                serde_json::to_string(&entry.payload)?,
                entry.payload_hash,
                entry.prev_hash,
                entry.entry_hash,
            ],
        )?;
        let project_path: String = conn
            .query_row(
                "SELECT project_path FROM sessions WHERE id = ?1",
                [session_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default();
        Self::search_sync_evidence_conn(&conn, &entry, &project_path)?;
        Ok(entry)
    }

    pub fn get_evidence(&self, session_id: Uuid) -> Result<Vec<EvidenceEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, seq, id, timestamp, kind, agent_id, payload_json, payload_hash, prev_hash, entry_hash
             FROM evidence WHERE session_id = ?1 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map([session_id.to_string()], evidence_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn evidence_head_hash(&self, session_id: Uuid) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        Ok(evidence_head(&conn, session_id)?.1)
    }

    pub fn verify_chain(&self, session_id: Uuid) -> Result<VerificationReport> {
        let entries = self.get_evidence(session_id)?;
        let mut expected_prev = GENESIS_HASH.to_string();
        let mut broken_at = Vec::new();
        for (expected_seq, entry) in (1u64..).zip(&entries) {
            if entry.seq != expected_seq || !entry.verify(&expected_prev) {
                broken_at.push(entry.seq);
            }
            expected_prev = entry.entry_hash.clone();
        }
        Ok(VerificationReport {
            session_id,
            entries_checked: entries.len() as u64,
            valid: broken_at.is_empty(),
            broken_at,
            head_hash: expected_prev,
        })
    }

    /// Link a provenance record to the session it was resolved inside (UX2). Resolutions that
    /// happen before the session id exists (create_session) are linked right after creation.
    pub fn set_provenance_session(&self, provenance_id: Uuid, session_id: Uuid) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE provenance SET session_id = ?1 WHERE id = ?2",
            rusqlite::params![session_id.to_string(), provenance_id.to_string()],
        )?;
        Ok(())
    }

    /// Provenance for a session: records linked by session_id, plus legacy NULL rows matched by
    /// project + agent + the session's lifetime window (pre-linkage history).
    pub fn provenance_for_session(&self, session: &Session) -> Result<Vec<ProvenanceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, session_id, agent_id, context_json, trace_json, conflicts_json
             FROM provenance
             WHERE session_id = ?1
                OR (session_id IS NULL AND agent_id = ?2 AND timestamp >= ?3 AND timestamp <= ?4)
             ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![
                session.id.to_string(),
                session.agent_id,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
            ],
            provenance_from_row,
        )?;
        let mut out = Vec::new();
        for row in rows {
            let record = row?;
            if record.session_id.is_some() || record.context.project_path == session.project_path {
                out.push(record);
            }
        }
        Ok(out)
    }

    pub fn insert_provenance(&self, record: &ProvenanceRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO provenance (id, timestamp, session_id, agent_id, context_json, trace_json, conflicts_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                record.id.to_string(),
                record.timestamp.to_rfc3339(),
                record.session_id.map(|id| id.to_string()),
                record.agent_id,
                serde_json::to_string(&record.context)?,
                serde_json::to_string(&record.trace)?,
                serde_json::to_string(&record.conflicts)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_provenance(&self, id: Uuid) -> Result<Option<ProvenanceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, session_id, agent_id, context_json, trace_json, conflicts_json FROM provenance WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(provenance_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_provenance(&self, limit: usize) -> Result<Vec<ProvenanceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, session_id, agent_id, context_json, trace_json, conflicts_json FROM provenance ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], provenance_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn add_handoff(&self, session_id: Uuid, record: &HandoffRecord) -> Result<()> {
        let mut session = self
            .get_session(session_id)?
            .context("session not found for handoff")?;
        session.handoffs.push(record.clone());
        session.updated_at = Utc::now();
        self.upsert_session(&session)
    }

    pub fn mark_handoff_acknowledged(&self, session_id: Uuid, handoff_id: Uuid) -> Result<bool> {
        let Some(mut session) = self.get_session(session_id)? else {
            return Ok(false);
        };
        let Some(record) = session.handoffs.iter_mut().find(|h| h.id == handoff_id) else {
            return Ok(false);
        };
        record.acknowledged = true;
        session.updated_at = Utc::now();
        self.upsert_session(&session)?;
        Ok(true)
    }

    pub fn upsert_project(&self, project: &Project) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, description, created_at, last_opened_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                path = excluded.path,
                description = excluded.description,
                last_opened_at = excluded.last_opened_at",
            rusqlite::params![
                project.id.to_string(),
                project.name,
                project.path.to_string_lossy(),
                project.description,
                project
                    .created_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
                project
                    .last_opened_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, description, created_at, last_opened_at FROM projects ORDER BY last_opened_at DESC",
        )?;
        let rows = stmt.query_map([], project_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_project(&self, id: Uuid) -> Result<Option<Project>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, description, created_at, last_opened_at FROM projects WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(project_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn project_by_path(&self, path: &str) -> Result<Option<Project>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, description, created_at, last_opened_at FROM projects WHERE path = ?1",
        )?;
        let mut rows = stmt.query([path])?;
        match rows.next()? {
            Some(row) => Ok(Some(project_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn touch_project(&self, id: Uuid) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET last_opened_at = ?1 WHERE id = ?2",
            rusqlite::params![
                Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
                id.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn delete_project(&self, id: Uuid) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute("DELETE FROM projects WHERE id = ?1", [id.to_string()])?;
        Ok(changed > 0)
    }

    pub fn insert_suggestion(&self, suggestion: &RuleSuggestion) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let params = suggestion_params(suggestion)?;
        conn.execute(
            "INSERT INTO rule_suggestions (id, project_path, agent_id, rule_id, title, topic, content, kind, tags_json, rationale, status, created_at, updated_at, reviewed_at, review_note, approved_rule_id, scope, enforcement)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        )?;
        Ok(())
    }

    pub fn update_suggestion(&self, suggestion: &RuleSuggestion) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let params = suggestion_params(suggestion)?;
        let changed = conn.execute(
            "UPDATE rule_suggestions SET
                project_path = ?2, agent_id = ?3, rule_id = ?4, title = ?5, topic = ?6,
                content = ?7, kind = ?8, tags_json = ?9, rationale = ?10, status = ?11,
                created_at = ?12, updated_at = ?13, reviewed_at = ?14, review_note = ?15,
                approved_rule_id = ?16, scope = ?17, enforcement = ?18
             WHERE id = ?1",
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        )?;
        Ok(changed > 0)
    }

    pub fn get_suggestion(&self, id: Uuid) -> Result<Option<RuleSuggestion>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("{SUGGESTION_SELECT} WHERE id = ?1"))?;
        let mut rows = stmt.query([id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(suggestion_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_suggestions(
        &self,
        project_path: Option<&str>,
        status: Option<SuggestionStatus>,
    ) -> Result<Vec<RuleSuggestion>> {
        let conn = self.conn.lock().unwrap();
        let (sql, params) = suggestion_filter_query(project_path, status, false);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            suggestion_from_row,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn count_suggestions(
        &self,
        project_path: Option<&str>,
        status: Option<SuggestionStatus>,
    ) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let (sql, params) = suggestion_filter_query(project_path, status, true);
        let count: i64 = conn.query_row(
            &sql,
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

const SUGGESTION_SELECT: &str = "SELECT id, project_path, agent_id, rule_id, title, topic, content, kind, tags_json, rationale, status, created_at, updated_at, reviewed_at, review_note, approved_rule_id, scope, enforcement FROM rule_suggestions";

fn suggestion_filter_query(
    project_path: Option<&str>,
    status: Option<SuggestionStatus>,
    count: bool,
) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
    let head = if count {
        "SELECT COUNT(*) FROM rule_suggestions".to_string()
    } else {
        SUGGESTION_SELECT.to_string()
    };
    let mut conditions: Vec<&str> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(path) = project_path {
        conditions.push("project_path = ?");
        params.push(Box::new(path.to_string()));
    }
    if let Some(status) = status {
        conditions.push("status = ?");
        params.push(Box::new(status.label().to_string()));
    }
    let mut sql = head;
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    if !count {
        sql.push_str(" ORDER BY created_at DESC");
    }
    (sql, params)
}

fn suggestion_params(suggestion: &RuleSuggestion) -> Result<Vec<Box<dyn rusqlite::types::ToSql>>> {
    Ok(vec![
        Box::new(suggestion.id.to_string()),
        Box::new(suggestion.project_path.to_string_lossy().to_string()),
        Box::new(suggestion.agent_id.clone()),
        Box::new(suggestion.rule_id.clone()),
        Box::new(suggestion.title.clone()),
        Box::new(suggestion.topic.clone()),
        Box::new(suggestion.content.clone()),
        Box::new(suggestion.kind.to_string()),
        Box::new(serde_json::to_string(&suggestion.tags)?),
        Box::new(suggestion.rationale.clone()),
        Box::new(suggestion.status.label().to_string()),
        Box::new(suggestion.created_at.to_rfc3339()),
        Box::new(suggestion.updated_at.to_rfc3339()),
        Box::new(suggestion.reviewed_at.map(|t| t.to_rfc3339())),
        Box::new(suggestion.review_note.clone()),
        Box::new(suggestion.approved_rule_id.clone()),
        Box::new(suggestion.scope.label().to_string()),
        Box::new(suggestion.enforcement.label().to_string()),
    ])
}

fn suggestion_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuleSuggestion> {
    let id: String = row.get(0)?;
    let kind_raw: String = row.get(7)?;
    let tags_json: String = row.get(8)?;
    let status_raw: String = row.get(10)?;
    let created: String = row.get(11)?;
    let updated: String = row.get(12)?;
    let reviewed: Option<String> = row.get(13)?;
    let scope_raw: String = row.get(16)?;
    let enforcement_raw: String = row.get(17)?;
    Ok(RuleSuggestion {
        id: Uuid::parse_str(&id).map_err(invalid_uuid)?,
        project_path: std::path::PathBuf::from(row.get::<_, String>(1)?),
        scope: serde_json::from_str(&format!("\"{scope_raw}\"")).map_err(json_err)?,
        agent_id: row.get(2)?,
        rule_id: row.get(3)?,
        title: row.get(4)?,
        topic: row.get(5)?,
        content: row.get(6)?,
        kind: serde_json::from_str(&format!("\"{kind_raw}\"")).map_err(json_err)?,
        enforcement: serde_json::from_str(&format!("\"{enforcement_raw}\"")).map_err(json_err)?,
        tags: serde_json::from_str(&tags_json).map_err(json_err)?,
        rationale: row.get(9)?,
        status: serde_json::from_str(&format!("\"{status_raw}\"")).map_err(json_err)?,
        created_at: parse_time(&created)?,
        updated_at: parse_time(&updated)?,
        reviewed_at: reviewed.map(|t| parse_time(&t)).transpose()?,
        review_note: row.get(14)?,
        approved_rule_id: row.get(15)?,
    })
}

fn evidence_head(conn: &Connection, session_id: Uuid) -> Result<(u64, String)> {
    let mut stmt = conn.prepare(
        "SELECT seq, entry_hash FROM evidence WHERE session_id = ?1 ORDER BY seq DESC LIMIT 1",
    )?;
    let mut rows = stmt.query([session_id.to_string()])?;
    match rows.next()? {
        Some(row) => {
            let seq: i64 = row.get(0)?;
            let hash: String = row.get(1)?;
            Ok((seq as u64, hash))
        }
        None => Ok((0, GENESIS_HASH.to_string())),
    }
}

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let id: String = row.get(0)?;
    let status_json: String = row.get(4)?;
    let created: String = row.get(5)?;
    let updated: String = row.get(6)?;
    let snapshot_json: String = row.get(7)?;
    let handoffs_json: String = row.get(8)?;
    let assurance_json: String = row.get(9)?;
    let goal: String = row.get(10)?;
    let work_items_json: String = row.get(11)?;
    Ok(Session {
        id: Uuid::parse_str(&id).map_err(invalid_uuid)?,
        task: row.get(1)?,
        project_path: std::path::PathBuf::from(row.get::<_, String>(2)?),
        agent_id: row.get(3)?,
        status: serde_json::from_str(&status_json).map_err(json_err)?,
        created_at: parse_time(&created)?,
        updated_at: parse_time(&updated)?,
        constraint_snapshot: serde_json::from_str(&snapshot_json).map_err(json_err)?,
        handoffs: serde_json::from_str(&handoffs_json).map_err(json_err)?,
        assurance: serde_json::from_str(&assurance_json).map_err(json_err)?,
        goal,
        work_items: serde_json::from_str(&work_items_json).map_err(json_err)?,
        harness_session_id: row.get(12)?,
        predecessor: row
            .get::<_, Option<String>>(13)?
            .map(|s| Uuid::parse_str(&s).map_err(invalid_uuid))
            .transpose()?,
        title: row.get(14)?,
        work_state_version: row.get::<_, i64>(15)? as u64,
    })
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let id: String = row.get(0)?;
    let created: String = row.get(4)?;
    let opened: String = row.get(5)?;
    Ok(Project {
        id: Uuid::parse_str(&id).map_err(invalid_uuid)?,
        name: row.get(1)?,
        path: std::path::PathBuf::from(row.get::<_, String>(2)?),
        description: row.get(3)?,
        created_at: parse_time(&created)?,
        last_opened_at: parse_time(&opened)?,
    })
}

fn evidence_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceEntry> {
    let session_id: String = row.get(0)?;
    let id: String = row.get(2)?;
    let timestamp: String = row.get(3)?;
    let kind_json: String = row.get(4)?;
    let payload_json: String = row.get(6)?;
    Ok(EvidenceEntry {
        seq: row.get::<_, i64>(1)? as u64,
        id: Uuid::parse_str(&id).map_err(invalid_uuid)?,
        session_id: Uuid::parse_str(&session_id).map_err(invalid_uuid)?,
        timestamp: parse_time(&timestamp)?,
        kind: serde_json::from_str(&kind_json).map_err(json_err)?,
        agent_id: row.get(5)?,
        payload: serde_json::from_str(&payload_json).map_err(json_err)?,
        payload_hash: row.get(7)?,
        prev_hash: row.get(8)?,
        entry_hash: row.get(9)?,
    })
}

fn provenance_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProvenanceRecord> {
    let id: String = row.get(0)?;
    let timestamp: String = row.get(1)?;
    let session_id: Option<String> = row.get(2)?;
    let context_json: String = row.get(4)?;
    let trace_json: String = row.get(5)?;
    let conflicts_json: String = row.get(6)?;
    Ok(ProvenanceRecord {
        id: Uuid::parse_str(&id).map_err(invalid_uuid)?,
        timestamp: parse_time(&timestamp)?,
        session_id: session_id
            .map(|s| Uuid::parse_str(&s).map_err(invalid_uuid))
            .transpose()?,
        agent_id: row.get(3)?,
        context: serde_json::from_str::<QueryContext>(&context_json).map_err(json_err)?,
        trace: serde_json::from_str::<Vec<RuleMatch>>(&trace_json).map_err(json_err)?,
        conflicts: serde_json::from_str::<Vec<Conflict>>(&conflicts_json).map_err(json_err)?,
    })
}

fn parse_time(s: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn invalid_uuid(e: uuid::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

fn json_err(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

pub fn status_label(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "active",
        SessionStatus::HandedOff => "handed_off",
        SessionStatus::Completed => "completed",
        SessionStatus::Failed => "failed",
    }
}
