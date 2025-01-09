use std::fs;
pub(crate) use std::{error::Error, path::Path};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::{config::Config, files::{DownloadStatus, File}};

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
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Chunk {
    pub id: i64,
    pub chunk_position: i64,
    pub record_id: i64,
    pub start :u64,
    pub end: u64,
    pub status: String,
}

impl Chunk {
    pub fn new(chunk_position: i64, record_id: i64, start: u64, end: u64, status: DownloadStatus) -> Self {
        let status = status.to_string();
        let id = 0;
        Chunk {
            id,
            chunk_position,
            record_id,
            start,
            end,
            status,
        }
    }
}

fn get_db(cfg: &Config) -> Result<Connection, Box<dyn std::error::Error>> {
    let db_path = Path::new(&cfg.config_dir);
    fs::create_dir_all(&db_path)?;
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

pub fn create_table(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let conn = get_db(&cfg)?;

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
           chunk_position   INTEGER NOT NULL,
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
    create_table(cfg)?;
    let conn = get_db(&cfg)?;

    let sql = r#"SELECT 
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
        })
    })?;
    let mut records = Vec::new();
    for r in record_iter {
        records.push(r?);
    }
    Ok(records)
}

pub fn search_by_url(
    url: &str,
    cfg: &Config,
) -> Result<DownloadRecord, Box<dyn std::error::Error>> {
    create_table(cfg)?;

    let conn = get_db(&cfg)?;
    let sql = r#"SELECT 
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
        })
    })?;
    Ok(record)
}

pub fn insert_record(
    record: &DownloadRecord,
    cfg: &Config,
) -> Result<i64, Box<dyn std::error::Error>> {
    create_table(cfg)?;

    let conn = get_db(&cfg)?;

    let sql = r#"INSERT INTO download_record (
            file_url, file_name, file_type, extension, destination_dir, 
            destination_path, file_size, download_start_time, download_stop_time, 
            download_status
            )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#;
    conn.execute(
        sql,
        params![
            record.file_url,
            record.file_name,
            record.file_type,
            record.extension,
            record.destination_dir,
            record.destination_path,
            record.file_size,
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
) -> Result<(), Box<dyn std::error::Error>> {
    create_table(cfg)?;
    let conn = get_db(&cfg)?;
    let sql = r#"UPDATE download_record 
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
            println!("FAILED TO UPDATE BECAUSE {}", e);
        }
    };
    Ok(())
}

pub fn delete_record(id: i64, cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    create_table(cfg)?;
    let conn = get_db(&cfg)?;
    let sql = "DELETE FROM download_record WHERE id=?1 LIMIT 1;";
    conn.execute(sql, params![id])?;
    Ok(())
}
