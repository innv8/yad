//! SQLite storage. One connection, WAL mode, schema versioned with `PRAGMA user_version`.
//!
//! Version 0 is the YAD 1.x schema (`download_record` + `chunk`); it is migrated in place.

use rusqlite::{params, Connection, OptionalExtension, Row};
use std::{path::Path, sync::Mutex};

use crate::{
    files::FileType,
    protocol::{Download, Headers, Kind, Settings, Source, Status},
};

pub type Result<T> = std::result::Result<T, rusqlite::Error>;

const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone)]
pub struct DownloadRow {
    pub download: Download,
    pub headers: Headers,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Stream variant chosen at add time (`Download::variant` holds its label).
    pub variant_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentRow {
    pub idx: i64,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
    pub done: bool,
}

pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Storage> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Storage> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Storage> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&conn)?;
        Ok(Storage { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves the connection usable; recover it.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, row: &DownloadRow) -> Result<i64> {
        let c = self.conn();
        let d = &row.download;
        c.execute(
            "INSERT INTO downloads (url, file_name, file_type, kind, destination_dir, destination_path,
                total_size, downloaded, status, error, resumable, retries, source, page_url, variant,
                headers, etag, last_modified, created_at, finished_at, variant_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
            params![
                d.url, d.file_name, d.file_type.as_str(), d.kind.as_str(), d.destination_dir,
                d.destination_path, d.total_size.map(|v| v as i64), d.downloaded as i64,
                d.status.as_str(), d.error, d.resumable, d.retries, d.source.as_str(), d.page_url,
                d.variant, serde_json::to_string(&row.headers).unwrap_or_default(), row.etag,
                row.last_modified, d.created_at as i64, d.finished_at.map(|v| v as i64), row.variant_id
            ],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn save(&self, row: &DownloadRow) -> Result<()> {
        let d = &row.download;
        self.conn().execute(
            "UPDATE downloads SET url=?2, file_name=?3, file_type=?4, kind=?5, destination_dir=?6,
                destination_path=?7, total_size=?8, downloaded=?9, status=?10, error=?11, resumable=?12,
                retries=?13, source=?14, page_url=?15, variant=?16, headers=?17, etag=?18,
                last_modified=?19, finished_at=?20, variant_id=?21
             WHERE id=?1",
            params![
                d.id, d.url, d.file_name, d.file_type.as_str(), d.kind.as_str(), d.destination_dir,
                d.destination_path, d.total_size.map(|v| v as i64), d.downloaded as i64,
                d.status.as_str(), d.error, d.resumable, d.retries, d.source.as_str(), d.page_url,
                d.variant, serde_json::to_string(&row.headers).unwrap_or_default(), row.etag,
                row.last_modified, d.finished_at.map(|v| v as i64), row.variant_id
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<DownloadRow>> {
        self.conn()
            .query_row(&format!("SELECT {COLUMNS} FROM downloads WHERE id=?1"), [id], read_row)
            .optional()
    }

    /// Newest first.
    pub fn list(&self) -> Result<Vec<DownloadRow>> {
        let c = self.conn();
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM downloads ORDER BY id DESC"))?;
        let rows = stmt.query_map([], read_row)?.collect::<Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM downloads WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn segments(&self, id: i64) -> Result<Vec<SegmentRow>> {
        let c = self.conn();
        let mut stmt = c.prepare(
            "SELECT idx, start, end, downloaded, done FROM segments WHERE download_id=?1 ORDER BY idx",
        )?;
        let rows = stmt
            .query_map([id], |r| {
                Ok(SegmentRow {
                    idx: r.get(0)?,
                    start: r.get::<_, i64>(1)? as u64,
                    end: r.get::<_, i64>(2)? as u64,
                    downloaded: r.get::<_, i64>(3)? as u64,
                    done: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Saves a checkpoint: all segments plus the download's byte count, atomically.
    pub fn checkpoint(&self, id: i64, segments: &[SegmentRow], downloaded: u64, total: Option<u64>) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM segments WHERE download_id=?1", [id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO segments (download_id, idx, start, end, downloaded, done) VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
            for s in segments {
                stmt.execute(params![id, s.idx, s.start as i64, s.end as i64, s.downloaded as i64, s.done])?;
            }
        }
        tx.execute(
            "UPDATE downloads SET downloaded=?2, total_size=?3 WHERE id=?1",
            params![id, downloaded as i64, total.map(|v| v as i64)],
        )?;
        tx.commit()
    }

    pub fn clear_segments(&self, id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM segments WHERE download_id=?1", [id])?;
        Ok(())
    }

    pub fn settings(&self) -> Result<Option<Settings>> {
        let v: Option<String> = self
            .conn()
            .query_row("SELECT value FROM settings WHERE key='settings'", [], |r| r.get(0))
            .optional()?;
        Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES ('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(s).unwrap_or_default()],
        )?;
        Ok(())
    }

    /// After a crash or reboot nothing is downloading; put those back in the queue.
    pub fn requeue_interrupted(&self) -> Result<usize> {
        self.conn().execute("UPDATE downloads SET status='Queued' WHERE status='Downloading'", [])
    }
}

const COLUMNS: &str = "id, url, file_name, file_type, kind, destination_dir, destination_path, total_size,
    downloaded, status, error, resumable, retries, source, page_url, variant, headers, etag,
    last_modified, created_at, finished_at, variant_id";

fn read_row(r: &Row<'_>) -> Result<DownloadRow> {
    let headers: Option<String> = r.get(16)?;
    Ok(DownloadRow {
        download: Download {
            id: r.get(0)?,
            url: r.get(1)?,
            file_name: r.get(2)?,
            file_type: FileType::parse(&r.get::<_, String>(3)?),
            kind: Kind::parse(&r.get::<_, String>(4)?),
            destination_dir: r.get(5)?,
            destination_path: r.get(6)?,
            total_size: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            downloaded: r.get::<_, i64>(8)? as u64,
            status: Status::parse(&r.get::<_, String>(9)?),
            error: r.get(10)?,
            resumable: r.get(11)?,
            connections: 0,
            speed: 0,
            eta: None,
            retries: r.get(12)?,
            source: Source::parse(&r.get::<_, String>(13)?),
            page_url: r.get(14)?,
            variant: r.get(15)?,
            created_at: r.get::<_, i64>(19)? as u64,
            finished_at: r.get::<_, Option<i64>>(20)?.map(|v| v as u64),
        },
        headers: headers.and_then(|h| serde_json::from_str(&h).ok()).unwrap_or_default(),
        etag: r.get(17)?,
        last_modified: r.get(18)?,
        variant_id: r.get(21)?,
    })
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS downloads (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            url              TEXT NOT NULL,
            file_name        TEXT NOT NULL,
            file_type        TEXT NOT NULL,
            kind             TEXT NOT NULL DEFAULT 'file',
            destination_dir  TEXT NOT NULL,
            destination_path TEXT NOT NULL,
            total_size       INTEGER NULL,
            downloaded       INTEGER NOT NULL DEFAULT 0,
            status           TEXT NOT NULL,
            error            TEXT NULL,
            resumable        INTEGER NOT NULL DEFAULT 0,
            retries          INTEGER NOT NULL DEFAULT 0,
            source           TEXT NOT NULL DEFAULT 'app',
            page_url         TEXT NULL,
            variant          TEXT NULL,
            headers          TEXT NULL,
            etag             TEXT NULL,
            last_modified    TEXT NULL,
            created_at       INTEGER NOT NULL,
            finished_at      INTEGER NULL,
            variant_id       TEXT NULL
        );
        CREATE INDEX IF NOT EXISTS downloads_url ON downloads(url);
        CREATE TABLE IF NOT EXISTS segments (
            download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
            idx         INTEGER NOT NULL,
            start       INTEGER NOT NULL,
            end         INTEGER NOT NULL,
            downloaded  INTEGER NOT NULL DEFAULT 0,
            done        INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (download_id, idx)
        );
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    let has_v1: bool = tx
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='download_record'",
            [],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if has_v1 {
        migrate_v1(&tx)?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()
}

/// YAD 1.x stored 1 MB chunks per record. Finished records keep their history; unfinished
/// ones were written by the old engine (which could corrupt files on retry), so they are
/// marked Failed and restart from zero when retried.
fn migrate_v1(conn: &Connection) -> Result<()> {
    let has_chunks: bool = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='chunk'", [], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    let mut stmt = conn.prepare(
        "SELECT id, file_url, file_name, file_type, destination_dir, destination_path, file_size,
                download_start_time, download_stop_time
         FROM download_record ORDER BY id",
    )?;
    let old: Vec<(i64, String, String, String, String, String, Option<i64>, i64, Option<i64>)> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?))
        })?
        .collect::<Result<Vec<_>>>()?;
    drop(stmt);

    for (old_id, url, name, ftype, dir, path, size, start, stop) in old {
        let (pending, finished): (i64, i64) = if has_chunks {
            conn.query_row(
                "SELECT COALESCE(SUM(status != 'Finished'), 0), COALESCE(SUM(status = 'Finished'), 0)
                 FROM chunk WHERE record_id=?1",
                [old_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?
        } else {
            (0, 0)
        };
        let complete = finished > 0 && pending == 0;
        let size = size.filter(|s| *s > 0);
        conn.execute(
            "INSERT INTO downloads (url, file_name, file_type, kind, destination_dir, destination_path,
                total_size, downloaded, status, error, resumable, created_at, finished_at)
             VALUES (?1,?2,?3,'file',?4,?5,?6,?7,?8,?9,1,?10,?11)",
            params![
                url,
                name,
                FileType::parse(&ftype).as_str(),
                dir,
                path,
                size,
                if complete { size.unwrap_or(0) } else { 0 },
                if complete { "Completed" } else { "Failed" },
                if complete { None } else { Some("Interrupted before the update. Retry to download again.") },
                start,
                if complete { stop.or(Some(start)) } else { None },
            ],
        )?;
    }
    conn.execute_batch("DROP TABLE IF EXISTS chunk; DROP TABLE IF EXISTS download_record;")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample(url: &str) -> DownloadRow {
        DownloadRow {
            download: Download {
                id: 0,
                url: url.into(),
                file_name: "file.zip".into(),
                file_type: FileType::Compressed,
                kind: Kind::File,
                destination_dir: "/tmp".into(),
                destination_path: "/tmp/file.zip".into(),
                total_size: Some(100),
                downloaded: 0,
                status: Status::Queued,
                error: None,
                resumable: true,
                connections: 0,
                speed: 0,
                eta: None,
                retries: 0,
                source: Source::App,
                page_url: None,
                variant: None,
                created_at: 1,
                finished_at: None,
            },
            headers: Headers::from([("Referer".to_string(), "https://a".to_string())]),
            etag: Some("\"v1\"".into()),
            last_modified: None,
            variant_id: None,
        }
    }

    #[test]
    fn insert_get_save_delete() {
        let s = Storage::open_in_memory().unwrap();
        let id = s.insert(&sample("https://example.com/file.zip")).unwrap();
        let mut row = s.get(id).unwrap().unwrap();
        assert_eq!(row.download.url, "https://example.com/file.zip");
        assert_eq!(row.headers.get("Referer").unwrap(), "https://a");
        row.download.status = Status::Paused;
        row.download.downloaded = 50;
        s.save(&row).unwrap();
        assert_eq!(s.get(id).unwrap().unwrap().download.status, Status::Paused);
        s.delete(id).unwrap();
        assert!(s.get(id).unwrap().is_none());
    }

    #[test]
    fn checkpoint_replaces_segments_and_cascades() {
        let s = Storage::open_in_memory().unwrap();
        let id = s.insert(&sample("https://example.com/x")).unwrap();
        let segs = vec![
            SegmentRow { idx: 0, start: 0, end: 49, downloaded: 10, done: false },
            SegmentRow { idx: 1, start: 50, end: 99, downloaded: 50, done: true },
        ];
        s.checkpoint(id, &segs, 60, Some(100)).unwrap();
        s.checkpoint(id, &segs, 60, Some(100)).unwrap();
        assert_eq!(s.segments(id).unwrap(), segs, "no duplicate rows after repeated checkpoints");
        assert_eq!(s.get(id).unwrap().unwrap().download.downloaded, 60);
        s.delete(id).unwrap();
        assert!(s.segments(id).unwrap().is_empty());
    }

    #[test]
    fn requeue_interrupted_downloads() {
        let s = Storage::open_in_memory().unwrap();
        let mut row = sample("https://example.com/y");
        row.download.status = Status::Downloading;
        let id = s.insert(&row).unwrap();
        assert_eq!(s.requeue_interrupted().unwrap(), 1);
        assert_eq!(s.get(id).unwrap().unwrap().download.status, Status::Queued);
    }

    #[test]
    fn settings_round_trip() {
        let s = Storage::open_in_memory().unwrap();
        assert!(s.settings().unwrap().is_none());
        let mut set = Settings::default();
        set.max_active = 5;
        s.save_settings(&set).unwrap();
        assert_eq!(s.settings().unwrap().unwrap().max_active, 5);
    }

    #[test]
    fn migrates_yad_1_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("yad.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE download_record (id INTEGER PRIMARY KEY AUTOINCREMENT, file_url TEXT NOT NULL UNIQUE,
                    file_name TEXT NOT NULL, file_type TEXT NOT NULL, extension TEXT NOT NULL,
                    destination_dir TEXT NOT NULL, destination_path TEXT NOT NULL UNIQUE, file_size INTEGER NULL,
                    download_start_time INTEGER NOT NULL, download_stop_time INTEGER NULL, download_status TEXT NOT NULL);
                 CREATE TABLE chunk (id INTEGER PRIMARY KEY AUTOINCREMENT, record_id INTEGER NOT NULL,
                    start INTEGER NOT NULL, end INTEGER NOT NULL, status TEXT NOT NULL);
                 INSERT INTO download_record VALUES (1,'https://a/done.csv','done.csv','Documents','csv','/d','/d/done.csv',2048,10,20,'Finished');
                 INSERT INTO download_record VALUES (2,'https://a/half.iso','half.iso','Compressed','iso','/d','/d/half.iso',4096,30,NULL,'Pending');
                 INSERT INTO chunk (record_id,start,end,status) VALUES (1,0,1023,'Finished'),(1,1024,2047,'Finished'),
                    (2,0,2047,'Finished'),(2,2048,4095,'Pending'),(2,0,2047,'Pending');",
            )
            .unwrap();
        }
        let s = Storage::open(&path).unwrap();
        let rows = s.list().unwrap();
        assert_eq!(rows.len(), 2);
        let done = rows.iter().find(|r| r.download.file_name == "done.csv").unwrap();
        assert_eq!(done.download.status, Status::Completed);
        assert_eq!(done.download.downloaded, 2048);
        assert_eq!(done.download.finished_at, Some(20));
        let half = rows.iter().find(|r| r.download.file_name == "half.iso").unwrap();
        assert_eq!(half.download.status, Status::Failed);
        assert_eq!(half.download.downloaded, 0);
        drop(s);
        // Opening again is a no-op.
        let s = Storage::open(&path).unwrap();
        assert_eq!(s.list().unwrap().len(), 2);
    }
}
