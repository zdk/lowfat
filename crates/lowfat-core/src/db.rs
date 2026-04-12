use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

/// SQLite tracking database. Same schema as the bash version.
pub struct Db {
    conn: Connection,
}

/// A single tracked command execution.
pub struct TrackRecord {
    pub original_cmd: String,
    pub lowfat_cmd: String,
    pub raw: String,
    pub filtered: String,
    pub exec_time_ms: u64,
    pub project_path: String,
}

/// A single invocation record for the usage-history table.
pub struct InvocationRecord {
    pub command: String,
    pub subcommand: String,
    pub raw_tokens: u64,
    pub filtered_tokens: u64,
    pub had_plugin: bool,
    pub exit_code: i32,
}

/// One row of the plugin-candidates ranking.
#[derive(Debug)]
pub struct HistoryRow {
    pub command: String,
    pub subcommand: String,
    pub runs: u64,
    pub avg_raw_tokens: f64,
    pub savings_pct: f64,
    pub plugin_ratio: f64,
    pub score: f64,
}

/// Max rows retained in the `invocations` table. Oldest are evicted on insert.
const INVOCATIONS_CAP: i64 = 10_000;

/// Summary row from gain report.
#[derive(Debug)]
pub struct GainSummary {
    pub commands: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub saved_tokens: u64,
    pub savings_pct: f64,
}

/// Top command row from gain report.
#[derive(Debug)]
pub struct TopCommand {
    pub command: String,
    pub runs: u64,
    pub saved: i64,
    pub avg_pct: f64,
}

/// Session summary.
#[derive(Debug)]
pub struct SessionSummary {
    pub commands: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub saved_tokens: i64,
    pub savings_pct: f64,
    pub total_time_ms: u64,
}

impl Db {
    /// Open (or create) the tracking database.
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("history.db");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS commands (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                original_cmd TEXT NOT NULL,
                lowfat_cmd TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                saved_tokens INTEGER NOT NULL,
                savings_pct REAL NOT NULL,
                exec_time_ms INTEGER DEFAULT 0,
                project_path TEXT DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_timestamp ON commands(timestamp);
            CREATE INDEX IF NOT EXISTS idx_project ON commands(project_path, timestamp);

            CREATE TABLE IF NOT EXISTS audit (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                plugin_name TEXT NOT NULL,
                runtime_type TEXT NOT NULL,
                command TEXT NOT NULL,
                action TEXT NOT NULL,
                checksum TEXT DEFAULT '',
                details TEXT DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit(timestamp);

            CREATE TABLE IF NOT EXISTS invocations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                command TEXT NOT NULL,
                subcommand TEXT NOT NULL DEFAULT '',
                raw_tokens INTEGER NOT NULL,
                filtered_tokens INTEGER NOT NULL,
                had_plugin INTEGER NOT NULL,
                exit_code INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_invocations_cmd ON invocations(command, subcommand);",
        )?;
        Ok(Db { conn })
    }

    /// Record a command execution.
    pub fn track(&self, record: &TrackRecord) -> Result<()> {
        let in_tok = crate::tokens::estimate_tokens(&record.raw);
        let out_tok = crate::tokens::estimate_tokens(&record.filtered);
        let saved = in_tok as i64 - out_tok as i64;
        let pct = if in_tok > 0 {
            (saved as f64 / in_tok as f64) * 100.0
        } else {
            0.0
        };

        self.conn.execute(
            "INSERT INTO commands(timestamp, original_cmd, lowfat_cmd, input_tokens, output_tokens, saved_tokens, savings_pct, exec_time_ms, project_path)
             VALUES(datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                record.original_cmd,
                record.lowfat_cmd,
                in_tok as i64,
                out_tok as i64,
                saved,
                pct,
                record.exec_time_ms as i64,
                record.project_path,
            ],
        )?;
        Ok(())
    }

    /// Record an invocation for the usage-history ranking. Evicts oldest rows
    /// once the table grows past `INVOCATIONS_CAP`.
    pub fn record_invocation(&self, rec: &InvocationRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO invocations(timestamp, command, subcommand, raw_tokens, filtered_tokens, had_plugin, exit_code)
             VALUES(datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                rec.command,
                rec.subcommand,
                rec.raw_tokens as i64,
                rec.filtered_tokens as i64,
                rec.had_plugin as i64,
                rec.exit_code,
            ],
        )?;
        // id is monotonic (AUTOINCREMENT), so this trims only when over the cap.
        self.conn.execute(
            "DELETE FROM invocations WHERE id <= (SELECT MAX(id) - ?1 FROM invocations)",
            [INVOCATIONS_CAP],
        )?;
        Ok(())
    }

    /// Rank command+subcommand pairs as plugin candidates.
    /// Score = runs × avg_raw_tokens × (1 − savings_ratio) — i.e. called often,
    /// produces a lot, and lowfat is not yet shrinking it much.
    pub fn history_ranking(&self, limit: usize) -> Result<Vec<HistoryRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT command, subcommand,
                    COUNT(*) AS runs,
                    AVG(raw_tokens) AS avg_raw,
                    CASE WHEN SUM(raw_tokens) > 0
                         THEN 100.0 * (1.0 - 1.0 * SUM(filtered_tokens) / SUM(raw_tokens))
                         ELSE 0 END AS savings_pct,
                    AVG(had_plugin) AS plugin_ratio,
                    COUNT(*) * AVG(raw_tokens) *
                        (CASE WHEN SUM(raw_tokens) > 0
                              THEN 1.0 - 1.0 * SUM(filtered_tokens) / SUM(raw_tokens)
                              ELSE 0 END) AS score
             FROM invocations
             GROUP BY command, subcommand
             ORDER BY score DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(HistoryRow {
                command: row.get(0)?,
                subcommand: row.get(1)?,
                runs: row.get::<_, i64>(2)? as u64,
                avg_raw_tokens: row.get(3)?,
                savings_pct: row.get(4)?,
                plugin_ratio: row.get(5)?,
                score: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Lifetime gain summary.
    pub fn gain_summary(&self) -> Result<GainSummary> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(saved_tokens),0),
                    CASE WHEN SUM(input_tokens)>0
                      THEN ROUND(100.0*SUM(saved_tokens)/SUM(input_tokens),1) ELSE 0 END
             FROM commands",
        )?;
        let row = stmt.query_row([], |row| {
            Ok(GainSummary {
                commands: row.get::<_, i64>(0)? as u64,
                input_tokens: row.get::<_, i64>(1)? as u64,
                output_tokens: row.get::<_, i64>(2)? as u64,
                saved_tokens: row.get::<_, i64>(3)? as u64,
                savings_pct: row.get(4)?,
            })
        })?;
        Ok(row)
    }

    /// Top commands by tokens saved.
    pub fn top_commands(&self, limit: usize) -> Result<Vec<TopCommand>> {
        let mut stmt = self.conn.prepare(
            "SELECT original_cmd, COUNT(*), SUM(saved_tokens), ROUND(AVG(savings_pct),1)
             FROM commands GROUP BY original_cmd ORDER BY SUM(saved_tokens) DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(TopCommand {
                command: row.get(0)?,
                runs: row.get::<_, i64>(1)? as u64,
                saved: row.get(2)?,
                avg_pct: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Session summary since a given timestamp.
    pub fn session_summary(&self, since: &str) -> Result<SessionSummary> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*),
                    COALESCE(SUM(input_tokens),0),
                    COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(saved_tokens),0),
                    CASE WHEN SUM(input_tokens)>0
                      THEN ROUND(100.0*SUM(saved_tokens)/SUM(input_tokens),1) ELSE 0 END,
                    COALESCE(SUM(exec_time_ms),0)
             FROM commands WHERE timestamp >= ?1",
        )?;
        let row = stmt.query_row([since], |row| {
            Ok(SessionSummary {
                commands: row.get::<_, i64>(0)? as u64,
                input_tokens: row.get::<_, i64>(1)? as u64,
                output_tokens: row.get::<_, i64>(2)? as u64,
                saved_tokens: row.get(3)?,
                savings_pct: row.get(4)?,
                total_time_ms: row.get::<_, i64>(5)? as u64,
            })
        })?;
        Ok(row)
    }

    /// Record an audit event (plugin load, security check, etc.).
    pub fn audit(
        &self,
        plugin_name: &str,
        runtime_type: &str,
        command: &str,
        action: &str,
        checksum: &str,
        details: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO audit(timestamp, plugin_name, runtime_type, command, action, checksum, details)
             VALUES(datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![plugin_name, runtime_type, command, action, checksum, details],
        )?;
        Ok(())
    }

    /// Get recent audit entries.
    pub fn audit_log(&self, limit: usize) -> Result<Vec<AuditEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, plugin_name, runtime_type, command, action, checksum, details
             FROM audit ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(AuditEntry {
                timestamp: row.get(0)?,
                plugin_name: row.get(1)?,
                runtime_type: row.get(2)?,
                command: row.get(3)?,
                action: row.get(4)?,
                checksum: row.get(5)?,
                details: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

/// Audit log entry.
#[derive(Debug)]
pub struct AuditEntry {
    pub timestamp: String,
    pub plugin_name: String,
    pub runtime_type: String,
    pub command: String,
    pub action: String,
    pub checksum: String,
    pub details: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_create_and_track() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path()).unwrap();

        let record = TrackRecord {
            original_cmd: "git status".to_string(),
            lowfat_cmd: "lowfat git status".to_string(),
            raw: "a".repeat(100), // 25 tokens
            filtered: "a".repeat(40), // 10 tokens
            exec_time_ms: 50,
            project_path: "/tmp/test".to_string(),
        };
        db.track(&record).unwrap();

        let summary = db.gain_summary().unwrap();
        assert_eq!(summary.commands, 1);
        assert_eq!(summary.input_tokens, 25);
        assert_eq!(summary.output_tokens, 10);
        assert_eq!(summary.saved_tokens, 15);
    }

    #[test]
    fn invocations_evict_past_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path()).unwrap();

        // Insert cap + 5 rows, verify eviction keeps us at the cap.
        for i in 0..(super::INVOCATIONS_CAP + 5) {
            db.record_invocation(&InvocationRecord {
                command: "git".into(),
                subcommand: format!("s{i}"),
                raw_tokens: 100,
                filtered_tokens: 20,
                had_plugin: true,
                exit_code: 0,
            }).unwrap();
        }
        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM invocations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, super::INVOCATIONS_CAP);
    }

    #[test]
    fn history_ranking_orders_by_score() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path()).unwrap();

        // "cargo build": 5 × 2000 × 0.05 savings = score 500 (big but barely filtered)
        for _ in 0..5 {
            db.record_invocation(&InvocationRecord {
                command: "cargo".into(), subcommand: "build".into(),
                raw_tokens: 2000, filtered_tokens: 1900,
                had_plugin: false, exit_code: 0,
            }).unwrap();
        }
        // "git status": 10 × 30 × 0.9 savings = score 270 (small and well-filtered)
        for _ in 0..10 {
            db.record_invocation(&InvocationRecord {
                command: "git".into(), subcommand: "status".into(),
                raw_tokens: 30, filtered_tokens: 3,
                had_plugin: true, exit_code: 0,
            }).unwrap();
        }

        let ranking = db.history_ranking(10).unwrap();
        assert_eq!(ranking.len(), 2);
        assert_eq!(ranking[0].command, "cargo");
        assert_eq!(ranking[0].subcommand, "build");
        assert_eq!(ranking[1].command, "git");
    }

    #[test]
    fn top_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path()).unwrap();

        for _ in 0..3 {
            db.track(&TrackRecord {
                original_cmd: "git diff".to_string(),
                lowfat_cmd: "lowfat git diff".to_string(),
                raw: "a".repeat(100),
                filtered: "a".repeat(20),
                exec_time_ms: 10,
                project_path: "/tmp".to_string(),
            }).unwrap();
        }

        let top = db.top_commands(10).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].command, "git diff");
        assert_eq!(top[0].runs, 3);
    }
}
