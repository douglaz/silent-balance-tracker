use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BalanceRow {
    pub account_token: String,
    pub timestamp_utc: DateTime<Utc>,
    pub balance_usd: Option<f64>,
    pub msisdn: Option<String>,
    pub last_update: Option<String>,
    pub status: String,
    pub note: Option<String>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating db parent dir {}", parent.display()))?;
            }
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite at {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS balance_log (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                account_token   TEXT    NOT NULL,
                timestamp_utc   TEXT    NOT NULL,
                balance_usd     REAL,
                msisdn          TEXT,
                last_update     TEXT,
                status          TEXT    NOT NULL,
                note            TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_balance_log_token_time
                ON balance_log(account_token, timestamp_utc);
            "#,
        )?;
        Ok(())
    }

    pub fn insert_row(&self, row: &BalanceRow) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            r#"
            INSERT INTO balance_log
                (account_token, timestamp_utc, balance_usd, msisdn, last_update, status, note)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                row.account_token,
                row.timestamp_utc.to_rfc3339(),
                row.balance_usd,
                row.msisdn,
                row.last_update,
                row.status,
                row.note,
            ],
        )?;
        Ok(())
    }

    pub fn latest_for(&self, token: &str) -> Result<Option<BalanceRow>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let row = conn
            .query_row(
                r#"
                SELECT account_token, timestamp_utc, balance_usd, msisdn, last_update, status, note
                FROM balance_log
                WHERE account_token = ?1
                ORDER BY timestamp_utc DESC
                LIMIT 1
                "#,
                params![token],
                row_from_sqlite,
            )
            .optional()?;
        Ok(row)
    }

    pub fn history(
        &self,
        token: &str,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<BalanceRow>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (sql, since_str) = match since {
            Some(s) => (
                r#"SELECT account_token, timestamp_utc, balance_usd, msisdn, last_update, status, note
                   FROM balance_log
                   WHERE account_token = ?1 AND timestamp_utc >= ?2
                   ORDER BY timestamp_utc ASC
                   LIMIT ?3"#,
                Some(s.to_rfc3339()),
            ),
            None => (
                r#"SELECT account_token, timestamp_utc, balance_usd, msisdn, last_update, status, note
                   FROM balance_log
                   WHERE account_token = ?1
                   ORDER BY timestamp_utc ASC
                   LIMIT ?3"#,
                None,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(
                params![token, since_str.unwrap_or_default(), limit as i64],
                row_from_sqlite,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

fn row_from_sqlite(row: &rusqlite::Row<'_>) -> rusqlite::Result<BalanceRow> {
    let ts_str: String = row.get(1)?;
    let ts = DateTime::parse_from_rfc3339(&ts_str)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?
        .with_timezone(&Utc);
    Ok(BalanceRow {
        account_token: row.get(0)?,
        timestamp_utc: ts,
        balance_usd: row.get(2)?,
        msisdn: row.get(3)?,
        last_update: row.get(4)?,
        status: row.get(5)?,
        note: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> Result<(Db, tempfile::TempDir)> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("test.sqlite");
        Ok((Db::open(&path)?, dir))
    }

    #[test]
    fn insert_and_read_back() -> Result<()> {
        let (db, _dir) = temp_db()?;
        let now = Utc::now();
        db.insert_row(&BalanceRow {
            account_token: "AAA".into(),
            timestamp_utc: now,
            balance_usd: Some(12.34),
            msisdn: Some("+1234".into()),
            last_update: Some("2026-05-02 03:00".into()),
            status: "ok".into(),
            note: None,
        })?;
        let latest = db.latest_for("AAA")?.expect("row inserted");
        assert_eq!(latest.account_token, "AAA");
        assert_eq!(latest.balance_usd, Some(12.34));
        assert!(db.latest_for("BBB")?.is_none());
        Ok(())
    }

    #[test]
    fn history_filters_by_token() -> Result<()> {
        let (db, _dir) = temp_db()?;
        let now = Utc::now();
        for (token, bal) in [("AAA", 1.0), ("BBB", 2.0), ("AAA", 3.0)] {
            db.insert_row(&BalanceRow {
                account_token: token.into(),
                timestamp_utc: now,
                balance_usd: Some(bal),
                msisdn: None,
                last_update: None,
                status: "ok".into(),
                note: None,
            })?;
        }
        assert_eq!(db.history("AAA", None, 100)?.len(), 2);
        assert_eq!(db.history("BBB", None, 100)?.len(), 1);
        Ok(())
    }
}
