//! SQLite 落盘存储：
//! - 单写线程（std mpsc 串行化写入），读走独立连接（WAL 模式读写不互斥）
//! - 大 body（>128KB）落文件，DB 存引用；读取时按需加载
//! - 定期裁剪，仅保留最新 MAX_RECORDS 条

use super::TrafficStore;
use crate::models::{RequestMeta, RequestRecord, WsFrame};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// 超过该大小的 body 文本落文件而不是内联进 DB。
const INLINE_BODY_MAX: usize = 128 * 1024;
/// 最多保留的记录条数（超过裁剪最旧的）。
const MAX_RECORDS: i64 = 20_000;
/// 每写入多少条做一次裁剪。
const PRUNE_EVERY: u64 = 512;

/// 写线程消息。
enum WriteOp {
    Insert(Box<RequestRecord>),
    FinishWs {
        id: String,
        duration_ms: u128,
        error: Option<String>,
    },
    Frame {
        record_id: String,
        frame: WsFrame,
    },
    Clear,
}

/// SQLite 存储。
pub struct SqliteStore {
    writer: Sender<WriteOp>,
    read: Mutex<Connection>,
    bodies_dir: PathBuf,
    inserts_since_prune: AtomicU64,
}

impl SqliteStore {
    /// 打开（或创建）数据库。db_path 为 sqlite 文件路径，bodies_dir 为大 body 文件目录。
    pub fn open(db_path: &Path, bodies_dir: &Path) -> Result<Arc<Self>, String> {
        std::fs::create_dir_all(bodies_dir).map_err(|e| format!("创建 body 目录失败：{e}"))?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建数据目录失败：{e}"))?;
        }

        // 初始化 schema（用临时连接）
        let init = Connection::open(db_path).map_err(|e| format!("打开数据库失败：{e}"))?;
        init.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        init.execute_batch(SCHEMA_SQL).map_err(|e| format!("初始化表失败：{e}"))?;
        migrate(&init);
        drop(init);

        // 写线程：独立连接 + 外键约束
        let wconn = Connection::open(db_path).map_err(|e| e.to_string())?;
        wconn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
        wconn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| e.to_string())?;
        wconn.pragma_update(None, "foreign_keys", "ON").map_err(|e| e.to_string())?;
        let w_bodies = bodies_dir.to_path_buf();

        let (tx, rx) = channel::<WriteOp>();
        thread::Builder::new()
            .name("paxi-sqlite-writer".into())
            .spawn(move || {
                for op in rx {
                    let _ = apply_write(&wconn, &w_bodies, op);
                }
            })
            .map_err(|e| format!("启动写线程失败：{e}"))?;

        // 读连接
        let rconn = Connection::open(db_path).map_err(|e| e.to_string())?;
        rconn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
        rconn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| e.to_string())?;

        Ok(Arc::new(Self {
            writer: tx,
            read: Mutex::new(rconn),
            bodies_dir: bodies_dir.to_path_buf(),
            inserts_since_prune: AtomicU64::new(0),
        }))
    }

    fn queue(&self, op: WriteOp) {
        let _ = self.writer.send(op);
    }
}

/// 写线程应用一条写操作。
fn apply_write(conn: &Connection, bodies_dir: &Path, op: WriteOp) -> Result<(), rusqlite::Error> {
    match op {
        WriteOp::Insert(rec) => {
            let (req_inline, req_file) = spill_body(bodies_dir, &rec.id, "req", rec.request_body.as_deref());
            let (resp_inline, resp_file) = spill_body(bodies_dir, &rec.id, "resp", rec.response_body.as_deref());
            let req_headers = serde_json::to_string(&rec.request_headers).unwrap_or_else(|_| "[]".into());
            let resp_headers = serde_json::to_string(&rec.response_headers).unwrap_or_else(|_| "[]".into());
            conn.execute(
                "INSERT OR REPLACE INTO record
                 (id, client_ip, client_process, method, url, host, scheme, status,
                  req_headers, resp_headers, req_body, resp_body, req_body_file, resp_body_file,
                  req_size, resp_size, content_type, duration_ms, started_at,
                  error, is_websocket, ws_frame_count, matched_rule, is_replay, passthrough)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                params![
                    rec.id,
                    rec.client_ip,
                    rec.client_process,
                    rec.method,
                    rec.url,
                    rec.host,
                    rec.scheme,
                    rec.status as i64,
                    req_headers,
                    resp_headers,
                    req_inline,
                    resp_inline,
                    req_file,
                    resp_file,
                    rec.request_body_size as i64,
                    rec.response_body_size as i64,
                    rec.content_type,
                    rec.duration_ms as i64,
                    rec.started_at as i64,
                    rec.error,
                    rec.is_websocket as i64,
                    rec.ws_frame_count as i64,
                    rec.matched_rule,
                    rec.is_replay as i64,
                    rec.passthrough as i64,
                ],
            )?;
        }
        WriteOp::FinishWs { id, duration_ms, error } => {
            conn.execute(
                "UPDATE record SET duration_ms = ?2, error = ?3 WHERE id = ?1",
                params![id, duration_ms as i64, error],
            )?;
        }
        WriteOp::Frame { record_id, frame } => {
            conn.execute(
                "INSERT OR REPLACE INTO ws_frame (record_id, seq, dir, opcode, payload_len, payload_text, ts_ms)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    record_id,
                    frame.seq as i64,
                    frame.dir as i64,
                    frame.opcode,
                    frame.payload_len as i64,
                    frame.payload_text,
                    frame.ts_ms as i64,
                ],
            )?;
            conn.execute(
                "UPDATE record SET ws_frame_count = (SELECT COUNT(*) FROM ws_frame WHERE record_id = ?1) WHERE id = ?1",
                params![record_id],
            )?;
        }
        WriteOp::Clear => {
            conn.execute("DELETE FROM record", [])?;
            // 清空 body 文件目录
            if let Ok(entries) = std::fs::read_dir(bodies_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
    }
    Ok(())
}

/// 轻量迁移：为旧库补齐后加的列（存在则跳过）。
fn migrate(conn: &Connection) {
    for col in ["matched_rule", "is_replay", "passthrough", "client_process"] {
        let has = conn
            .prepare(&format!(
                "SELECT COUNT(*) FROM pragma_table_info('record') WHERE name = '{col}'"
            ))
            .and_then(|mut s| s.query_row([], |r| r.get::<_, i64>(0)))
            .unwrap_or(0);
        if has == 0 {
            let default = if col == "matched_rule" || col == "client_process" {
                " TEXT"
            } else {
                " INTEGER NOT NULL DEFAULT 0"
            };
            let _ = conn.execute(&format!("ALTER TABLE record ADD COLUMN {col}{default}"), []);
        }
    }
}

/// 大 body 落文件，返回 (内联文本, 文件相对路径) 二选一。
fn spill_body(
    bodies_dir: &Path,
    id: &str,
    dir_tag: &str,
    body: Option<&str>,
) -> (Option<String>, Option<String>) {
    let Some(text) = body else {
        return (None, None);
    };
    if text.len() <= INLINE_BODY_MAX {
        return (Some(text.to_string()), None);
    }
    let file_name = format!("{id}_{dir_tag}.txt");
    let path = bodies_dir.join(&file_name);
    if std::fs::write(&path, text).is_ok() {
        (None, Some(file_name))
    } else {
        // 写文件失败则退回内联（可能较大，但保证不丢）
        (Some(text.to_string()), None)
    }
}

/// 按引用读取 body：内联直接返回，文件则读回。
fn load_body(bodies_dir: &Path, inline: Option<String>, file: Option<String>) -> Option<String> {
    if let Some(text) = inline {
        return Some(text);
    }
    if let Some(name) = file {
        return std::fs::read_to_string(bodies_dir.join(name)).ok();
    }
    None
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS record (
    id             TEXT PRIMARY KEY,
    client_ip      TEXT,
    method         TEXT NOT NULL,
    url            TEXT NOT NULL,
    host           TEXT NOT NULL,
    scheme         TEXT NOT NULL,
    status         INTEGER NOT NULL DEFAULT 0,
    req_headers    TEXT NOT NULL DEFAULT '[]',
    resp_headers   TEXT NOT NULL DEFAULT '[]',
    req_body       TEXT,
    resp_body      TEXT,
    req_body_file  TEXT,
    resp_body_file TEXT,
    req_size       INTEGER NOT NULL DEFAULT 0,
    resp_size      INTEGER NOT NULL DEFAULT 0,
    content_type   TEXT,
    duration_ms    INTEGER NOT NULL DEFAULT 0,
    started_at     INTEGER NOT NULL,
    error          TEXT,
    is_websocket   INTEGER NOT NULL DEFAULT 0,
    ws_frame_count INTEGER NOT NULL DEFAULT 0,
    matched_rule   TEXT,
    is_replay      INTEGER NOT NULL DEFAULT 0,
    passthrough    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_record_started ON record(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_record_host ON record(host);
CREATE TABLE IF NOT EXISTS ws_frame (
    record_id    TEXT NOT NULL REFERENCES record(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    dir          INTEGER NOT NULL,
    opcode       TEXT NOT NULL,
    payload_len  INTEGER NOT NULL DEFAULT 0,
    payload_text TEXT,
    ts_ms        INTEGER NOT NULL,
    PRIMARY KEY (record_id, seq)
);
"#;

impl TrafficStore for SqliteStore {
    fn insert(&self, record: RequestRecord) -> RequestMeta {
        let meta = RequestMeta::from(&record);
        self.queue(WriteOp::Insert(Box::new(record)));

        // 周期性裁剪
        let n = self.inserts_since_prune.fetch_add(1, Ordering::Relaxed) + 1;
        if n % PRUNE_EVERY == 0 {
            if let Ok(conn) = self.read.lock() {
                let _ = conn.execute(
                    "DELETE FROM record WHERE id IN (
                        SELECT id FROM record ORDER BY started_at DESC LIMIT -1 OFFSET ?1
                    )",
                    params![MAX_RECORDS],
                );
            }
        }
        meta
    }

    fn finish_websocket(&self, id: &str, duration_ms: u128, error: Option<String>) {
        self.queue(WriteOp::FinishWs {
            id: id.to_string(),
            duration_ms,
            error,
        });
    }

    fn insert_frame(&self, record_id: &str, frame: WsFrame) {
        self.queue(WriteOp::Frame {
            record_id: record_id.to_string(),
            frame,
        });
    }

    fn list(&self) -> Vec<RequestMeta> {
        let Ok(conn) = self.read.lock() else {
            return vec![];
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, method, url, host, scheme, status, duration_ms, started_at,
                    is_websocket, ws_frame_count, error, client_ip, client_process,
                    req_size, resp_size, content_type, is_replay, passthrough
             FROM record ORDER BY started_at DESC, rowid DESC LIMIT ?1",
        ) else {
            return vec![];
        };
        let rows = stmt.query_map(params![MAX_RECORDS], |row| {
            Ok(RequestMeta {
                id: row.get(0)?,
                method: row.get(1)?,
                url: row.get(2)?,
                host: row.get(3)?,
                scheme: row.get(4)?,
                status: row.get::<_, i64>(5)? as u16,
                duration_ms: row.get::<_, i64>(6)? as u128,
                started_at: row.get::<_, i64>(7)? as u128,
                is_websocket: row.get::<_, i64>(8)? != 0,
                ws_frame_count: row.get::<_, i64>(9)? as u64,
                error: row.get(10)?,
                client_ip: row.get(11)?,
                client_process: row.get(12)?,
                request_body_size: row.get::<_, i64>(13)? as u64,
                response_body_size: row.get::<_, i64>(14)? as u64,
                content_type: row.get(15)?,
                is_replay: row.get::<_, i64>(16)? != 0,
                passthrough: row.get::<_, i64>(17)? != 0,
            })
        });
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => vec![],
        }
    }

    fn get(&self, id: &str) -> Option<RequestRecord> {
        let conn = self.read.lock().ok()?;
        let mut stmt = conn
            .prepare(
                "SELECT client_ip, client_process, method, url, host, scheme, status,
                        req_headers, resp_headers, req_body, resp_body, req_body_file, resp_body_file,
                        req_size, resp_size, content_type, duration_ms, started_at,
                        error, is_websocket, ws_frame_count, matched_rule, is_replay, passthrough
                 FROM record WHERE id = ?1",
            )
            .ok()?;
        let row = stmt
            .query_row(params![id], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)? as u16,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)? as u64,
                    row.get::<_, i64>(14)? as u64,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, i64>(16)? as u128,
                    row.get::<_, i64>(17)? as u128,
                    row.get::<_, Option<String>>(18)?,
                    row.get::<_, i64>(19)? != 0,
                    row.get::<_, i64>(20)? as u64,
                    row.get::<_, Option<String>>(21)?,
                    row.get::<_, i64>(22)? != 0,
                    row.get::<_, i64>(23)? != 0,
                ))
            })
            .ok()?;

        let (client_ip, client_process, method, url, host, scheme, status, req_h, resp_h, req_inline, resp_inline, req_file, resp_file, req_size, resp_size, content_type, duration_ms, started_at, error, is_websocket, ws_frame_count, matched_rule, is_replay, passthrough) = row;

        let request_headers: Vec<(String, String)> =
            serde_json::from_str(&req_h).unwrap_or_default();
        let response_headers: Vec<(String, String)> =
            serde_json::from_str(&resp_h).unwrap_or_default();

        Some(RequestRecord {
            id: id.to_string(),
            client_ip,
            method,
            url,
            host,
            scheme,
            status,
            request_headers,
            response_headers,
            request_body: load_body(&self.bodies_dir, req_inline, req_file),
            response_body: load_body(&self.bodies_dir, resp_inline, resp_file),
            request_body_size: req_size,
            response_body_size: resp_size,
            content_type,
            duration_ms,
            started_at,
            error,
            is_websocket,
            ws_frame_count,
            matched_rule,
            is_replay,
            passthrough,
            client_process,
        })
    }

    fn frames(&self, id: &str) -> Vec<WsFrame> {
        let Ok(conn) = self.read.lock() else {
            return vec![];
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT seq, dir, opcode, payload_len, payload_text, ts_ms
             FROM ws_frame WHERE record_id = ?1 ORDER BY seq ASC",
        ) else {
            return vec![];
        };
        let rows = stmt.query_map(params![id], |row| {
            Ok(WsFrame {
                seq: row.get::<_, i64>(0)? as u64,
                dir: row.get::<_, i64>(1)? as u8,
                opcode: row.get(2)?,
                payload_len: row.get::<_, i64>(3)? as u64,
                payload_text: row.get(4)?,
                ts_ms: row.get::<_, i64>(5)? as u128,
            })
        });
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => vec![],
        }
    }

    fn clear(&self) {
        self.queue(WriteOp::Clear);
    }
}
