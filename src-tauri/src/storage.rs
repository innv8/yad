use std::fs;
use std::{error::Error, path::Path};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::{
    config::Config,
    files::{DownloadStatus, File},
};

#[derive(Debug, Clone, Serialize, Default)]
pub struct DownloadRecord {
    pub id: i64,
    pub file_url: String,
    pub file_name: String,
    pub file_type: String,
    pub extension: String,
    pub destination_dir: String,
    pub destination_path: String,
    pub file_size: u64,
    pub download_start_time: u64,
    pub download_stop_time: u64,
    pub download_status: String,
    pub downloaded_percentage: f32,
}

impl From<File> for DownloadRecord {
    fn from(f: File) -> Self {
        DownloadRecord {
            id: 0,
            file_url: f.file_url,
            file_name: f.file_name,
            file_type: f.file_type.to_string(),
            extension: f.extension,
            destination_dir: f.destination_dir,
            destination_path: f.destination_path,
            file_size: f.file_size,
            download_start_time: f.download_start_time,
            download_stop_time: f.download_stop_time,
            download_status: f.download_status.to_string(),
            downloaded_percentage: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Chunk {
    pub id: i64,
    pub record_id: i64,
    pub start: u64,
    pub end: u64,
    pub status: String,
}

impl Chunk {
    pub fn new(record_id: i64, start: u64, end: u64) -> Self {
        let status = "InProgress".to_string();
        let id = 0;
        Chunk {
            id,
            record_id,
            start,
            end,
            status,
        }
    }
}

#[derive(Debug)]
struct ChunkCount {
    count: i32,
    status: String,
}

fn get_db(cfg: &Config) -> Result<Connection, Box<dyn Error>> {
    let db_path = Path::new(&cfg.config_dir);
    fs::create_dir_all(db_path)?;
    let db_path = db_path
        .join("yad.db")
        .to_str()
        .unwrap_or("/tmp/yad.db")
        .to_string();

    println!("db path: {}", &db_path);
    let conn = Connection::open(&db_path)?;

    // enable relationships in sqlite3
    conn.execute("PRAGMA foreign_keys = ON;", [])?;
    Ok(conn)
}

pub fn create_tables(cfg: &Config) -> Result<(), Box<dyn Error>> {
    let conn = get_db(cfg)?;

    let sql = r#"
        CREATE TABLE IF NOT EXISTS download_record (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            file_url            TEXT NOT NULL UNIQUE,
            file_name           TEXT NOT NULL,
            file_type           TEXT NOT NULL,
            extension           TEXT NOT NULL,
            destination_dir     TEXT NOT NULL,
            destination_path    TEXT NOT NULL UNIQUE,
            file_size           INTEGER NULL,
            download_start_time INTEGER NOT NULL,
            download_stop_time  INTEGER NULL,
            download_status     TEXT NOT NULL    
        )"#;
    conn.execute(sql, [])?;

    // create the child table for chunks
    let sql = r#"
        CREATE TABLE IF NOT EXISTS chunk (
           id               INTEGER PRIMARY KEY AUTOINCREMENT,
           record_id        INTEGER NOT NULL,
           start            INTEGER NOT NULL,
           end              INTEGER NOT NULL,
           status           TEXT NOT NULL,

           FOREIGN KEY (record_id) 
                REFERENCES download_record(id)
                ON DELETE CASCADE
        );
        "#;
    conn.execute(sql, [])?;
    Ok(())
}

pub fn read_download_records(cfg: &Config) -> Result<Vec<DownloadRecord>, Box<dyn Error>> {
    let conn = get_db(cfg)?;

    let sql = r#"
        SELECT 
            id, file_url, file_name, file_type, extension,
            destination_dir, destination_path, file_size,
            download_start_time, download_stop_time,
            download_status
        FROM download_record
        ORDER BY id DESC
        "#;
    let mut stmt = conn.prepare(sql)?;
    let record_iter = stmt.query_map([], |row| {
        Ok(DownloadRecord {
            id: row.get(0)?,
            file_url: row.get(1)?,
            file_name: row.get(2)?,
            file_type: row.get(3)?,
            extension: row.get(4)?,
            destination_dir: row.get(5)?,
            destination_path: row.get(6)?,
            file_size: row.get(7)?,
            download_start_time: row.get(8)?,
            download_stop_time: row.get(9)?,
            download_status: row.get(10)?,
            downloaded_percentage: 0.0,
        })
    })?;
    let mut records = Vec::new();
    for r in record_iter {
        let mut _r = r?;

        // check chunks and their statuses if the status == 'Pending'
        let (pending, finished, failed) = count_chunks(_r.id, cfg).unwrap();

        let downloaded_percentage: f32 = (finished as f32 / (pending + finished + failed) as f32) * 100.0;
        let mut status = "Pending";
        
        if failed > 0 {
            status = "Failed";
        } else if pending == 0 {
            status = "Finished";
        }

        _r.download_status = status.to_string();
        _r.downloaded_percentage = downloaded_percentage;

        // update the download record with the new status.
        // update_download_record(_r.id, status, _r.download_stop_time, _r.file_size, cfg).unwrap();


        records.push(_r);
    }
    Ok(records)
}

pub fn search_by_url(url: &str, cfg: &Config) -> Result<DownloadRecord, Box<dyn Error>> {
    let conn = get_db(cfg)?;
    let sql = r#"
        SELECT 
            id, file_url, file_name, file_type, extension,
            destination_dir, destination_path, file_size,
            download_start_time, download_stop_time,
            download_status
        FROM download_record
        WHERE file_url=?1
        LIMIT 1;
    "#;
    let record = conn.query_row(sql, params![url], |row| {
        Ok(DownloadRecord {
            id: row.get(0)?,
            file_url: row.get(1)?,
            file_name: row.get(2)?,
            file_type: row.get(3)?,
            extension: row.get(4)?,
            destination_dir: row.get(5)?,
            destination_path: row.get(6)?,
            file_size: row.get(7)?,
            download_start_time: row.get(8)?,
            download_stop_time: row.get(9)?,
            download_status: row.get(10)?,
            downloaded_percentage: 0.0,
        })
    })?;
    Ok(record)
}

pub fn insert_record(record: &DownloadRecord, file_size: u64, cfg: &Config) -> Result<i64, Box<dyn Error>> {
    let conn = get_db(cfg)?;

    let sql = r#"
        INSERT INTO download_record (
            file_url, file_name, file_type, extension, destination_dir, 
            destination_path, file_size, download_start_time, 
            download_stop_time, download_status
            )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#;
    conn.execute(
        sql,
        params![
            record.file_url,
            record.file_name,
            record.file_type,
            record.extension,
            record.destination_dir,
            record.destination_path,
            file_size,
            record.download_start_time,
            record.download_stop_time,
            record.download_status,
        ],
    )?;
    let id: i64 = conn.last_insert_rowid();

    Ok(id)
}

pub fn update_download_record(
    id: i64,
    download_status: &str,
    download_stop_time: u64,
    file_size: u64,
    cfg: &Config,
) -> Result<(), Box<dyn Error>> {
    let conn = get_db(cfg)?;
    let sql = r#"
        UPDATE download_record 
        SET download_status=?1, download_stop_time=?2, file_size=?3
        WHERE id = ?4
        LIMIT 1;"#;
    match conn.execute(
        sql,
        params![download_status, download_stop_time, file_size, id,],
    ) {
        Ok(_) => {
            println!("UPDATED SUCCESSFULLY");
        }
        Err(e) => {
            eprintln!("FAILED TO UPDATE BECAUSE {}", e);
        }
    };
    Ok(())
}

pub fn delete_record(id: i64, cfg: &Config) -> Result<(), Box<dyn Error>> {
    let conn = get_db(cfg)?;
    let sql = r#"
        DELETE FROM download_record 
        WHERE id=?1 LIMIT 1;
    "#;
    conn.execute(sql, params![id])?;
    Ok(())
}

pub fn save_chunk(chunk: &Chunk, cfg: &Config) -> Result<i64, Box<dyn Error>> {
    let conn = get_db(cfg)?;
    println!("======= inserting into chunk with data: {:?}", chunk);
    let sql = r#"
        INSERT INTO chunk (
            record_id, start, end, status
        )
        VALUES (?, ?, ?, ?)
        "#;
    conn.execute(
        sql,
        params![chunk.record_id, chunk.start, chunk.end, chunk.status],
    )?;
    let id: i64 = conn.last_insert_rowid();
    Ok(id)
}

pub fn update_chunk(
    record_id: i64,
    start: u64,
    status: &str,
    cfg: &Config,
) -> Result<(), Box<dyn Error>> {
    let conn = get_db(cfg)?;
    let sql = r#"
        UPDATE chunk 
        SET status=?1 
        WHERE record_id = ?2
            AND start = ?3
        LIMIT 1;
        "#;
    conn.execute(sql, params![status, record_id, start])?;
    Ok(())
}

pub fn fetch_chunks(record_id: i64, cfg: &Config) -> Result<Vec<Chunk>, Box<dyn Error>> {
    let conn = get_db(cfg)?;
    let sql = r#"
        SELECT (
            id, record_id, start, end, status
        )
        FROM chunk
        WHERE record_id = ?1 
        ORDER BY start ASC;
        "#;
    let mut stmt = conn.prepare(sql)?;
    let record_iter = stmt.query_map(params![record_id], |row| {
        Ok(Chunk {
            id: row.get(0)?,
            record_id: row.get(1)?,
            start: row.get(2)?,
            end: row.get(3)?,
            status: row.get(4)?,
        })
    })?;
    let mut chunks = Vec::new();
    for r in record_iter {
        chunks.push(r?);
    }
    Ok(chunks)
}

/// Count summaries of chunks for the files. We count how many chunks are pending, successful and
/// failed to determine the status and final state of the download.
///
/// # Arguments
/// - `record_id`: The download record id.
/// - `cfg`: Configs.
///
/// # Return
/// - `(i32, i32, i32)`: number of pending, successful and failed chunks.
pub fn count_chunks(record_id: i64, cfg: &Config) -> Result<(i32, i32, i32), Box<dyn Error>> {
    let conn = get_db(cfg)?;
    let sql = r#"
        SELECT COUNT(id), status
        FROM chunk
        WHERE record_id = ?1
        GROUP BY status;
        "#;
    let mut stmt = conn.prepare(sql)?;
    let record_iter = stmt.query_map(params![record_id], |row| {
        Ok(ChunkCount {
            count: row.get(0)?,
            status: row.get(1)?,
        })
    })?;

    let mut pending: i32 = 0;
    let mut finished: i32 = 0;
    let mut failed: i32 = 0;
    for record in record_iter {
        let r = record?;
        println!("status={}, count: {}", r.status, r.count);
        if r.status == "Pending" {
            pending = r.count;
        } else if r.status == "Finished" {
            finished = r.count;
        } else {
            failed = r.count;
        }
    }
    println!("---- pending: {pending}, finished: {finished}, failed: {failed}");
    Ok((pending, finished, failed))
}

