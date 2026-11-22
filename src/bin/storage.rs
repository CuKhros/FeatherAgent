//! `feather-storage`：storage 插件进程（设计文档 §6.1 / §6.2 / §6.3）。
//!
//! 内核不持有业务存储：这里才是 SQLite + FTS5 + 向量索引的家。
//! 向量三层：L0 热（进程内最近 5000 条，常驻）/ L1 温（按 model 分区从库加载）/
//! L2 冷（全库扫描，只用于显式全库搜索）。
//!
//! 落盘：`FEATHER_DATA/index/index.db`（WAL + synchronous=NORMAL）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use featheragent::proto::types::PluginManifest;
use featheragent::proto::{read_frame, write_frame, Codec};

const L0_LIMIT: usize = 5_000;

struct VecItem {
    id: String,
    session: String,
    model: String,
    vector: Vec<f32>,
    text: String,
    ts: i64,
}

impl VecItem {
    fn bytes(&self) -> u64 {
        (self.vector.len() * 4 + self.text.len() + self.id.len() + 64) as u64
    }
}

struct Storage {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    l0: Mutex<Vec<VecItem>>,
    l0_bytes: AtomicU64,
    budget: u64,
    evictions: AtomicU64,
}

impl Storage {
    fn open(data: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
        let index_dir = data.join("index");
        std::fs::create_dir_all(&index_dir).map_err(|e| e.to_string())?;
        let db_path = index_dir.join("index.db");
        let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS events(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               session TEXT NOT NULL, seq INTEGER NOT NULL, ts INTEGER NOT NULL,
               kind TEXT NOT NULL, text TEXT NOT NULL,
               UNIQUE(session, seq));
             CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
               text, session UNINDEXED, seq UNINDEXED, kind UNINDEXED, tokenize='unicode61');
             CREATE TABLE IF NOT EXISTS embeddings(
               id TEXT PRIMARY KEY, session TEXT, model TEXT, dim INTEGER,
               vector BLOB, text TEXT, ts INTEGER);
             CREATE INDEX IF NOT EXISTS idx_emb_model ON embeddings(model);",
        )
        .map_err(|e| e.to_string())?;
        let budget: u64 = std::env::var("VECTOR_MEM_BUDGET")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(128 * 1024 * 1024);
        let st = Self {
            conn: Mutex::new(conn),
            db_path,
            l0: Mutex::new(Vec::new()),
            l0_bytes: AtomicU64::new(0),
            budget,
            evictions: AtomicU64::new(0),
        };
        st.load_l0()?;
        Ok(st)
    }

    /// L0 启动加载：最近 5000 条向量常驻（§6.3）。
    fn load_l0(&self) -> Result<(), String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, session, model, dim, vector, text, ts FROM embeddings ORDER BY ts DESC LIMIT ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![L0_LIMIT as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect::<Vec<_>>();
        let mut l0 = self.l0.lock();
        for (id, session, model, _dim, blob, text, ts) in rows {
            let vector = decode_f32(&blob);
            let item = VecItem {
                id,
                session,
                model,
                vector,
                text,
                ts,
            };
            self.l0_bytes.fetch_add(item.bytes(), Ordering::Relaxed);
            l0.push(item);
        }
        Ok(())
    }

    fn evict_if_needed(&self) {
        let mut l0 = self.l0.lock();
        while self.l0_bytes.load(Ordering::Relaxed) > self.budget && !l0.is_empty() {
            let oldest = l0
                .iter()
                .enumerate()
                .min_by_key(|(_, x)| x.ts)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let removed = l0.remove(oldest);
            self.l0_bytes.fetch_sub(removed.bytes(), Ordering::Relaxed);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn index_append(&self, events: &[Value]) -> Result<Value, String> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let mut inserted = 0u64;
        for e in events {
            let session = e.get("session").and_then(|x| x.as_str()).unwrap_or("");
            let seq = e.get("seq").and_then(|x| x.as_u64()).unwrap_or(0) as i64;
            let ts = e.get("ts").and_then(|x| x.as_u64()).unwrap_or(0) as i64;
            let kind = e.get("kind").and_then(|x| x.as_str()).unwrap_or("");
            let payload = e.get("payload").cloned().unwrap_or(Value::Null);
            let text = extract_text(&payload);
            if text.is_empty() {
                continue;
            }
            let changed = tx
                .execute(
                    "INSERT OR IGNORE INTO events(session, seq, ts, kind, text) VALUES(?1,?2,?3,?4,?5)",
                    params![session, seq, ts, kind, text],
                )
                .map_err(|e| e.to_string())?;
            if changed == 0 {
                continue;
            }
            let id: i64 = tx
                .query_row(
                    "SELECT id FROM events WHERE session=?1 AND seq=?2",
                    params![session, seq],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO events_fts(rowid, text, session, seq, kind) VALUES(?1,?2,?3,?4,?5)",
                params![id, text, session, seq, kind],
            )
            .map_err(|e| e.to_string())?;
            inserted += 1;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(json!({"inserted": inserted}))
    }

    fn search_fts(&self, q: &str, limit: usize) -> Result<Value, String> {
        let conn = self.conn.lock();
        // FTS5 查询语法收紧：把引号转义，避免用户输入被当成运算符
        let safe = q.replace('"', " ");
        let safe = safe.trim();
        if safe.is_empty() {
            return Ok(json!({"items": []}));
        }
        let mut stmt = conn
            .prepare(
                "SELECT session, seq, kind, snippet(events_fts, 0, '[', ']', '…', 12) AS s,
                        bm25(events_fts) AS rank
                 FROM events_fts WHERE events_fts MATCH ?1 ORDER BY rank LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![format!("\"{safe}\""), limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, f64>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for (session, seq, kind, text, rank) in rows.filter_map(|x| x.ok()) {
            items.push(json!({
                "id": format!("{session}:{seq}"),
                "session": session, "seq": seq, "kind": kind,
                "text": text, "score": (-rank) as f32,
            }));
        }
        Ok(json!({"items": items}))
    }

    fn vec_upsert(&self, p: &Value) -> Result<Value, String> {
        let id = p.get("id").and_then(|x| x.as_str()).ok_or("缺少 id")?;
        let session = p.get("session").and_then(|x| x.as_str()).unwrap_or("");
        let model = p.get("model").and_then(|x| x.as_str()).unwrap_or("default");
        let text = p.get("text").and_then(|x| x.as_str()).unwrap_or("");
        let vector: Vec<f32> = p
            .get("vector")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_f64().map(|f| f as f32))
                    .collect()
            })
            .unwrap_or_default();
        if vector.is_empty() {
            return Err("向量为空".into());
        }
        let ts = p
            .get("ts")
            .and_then(|x| x.as_u64())
            .unwrap_or_else(|| featheragent::proto::types::now_wall_ns()) as i64;
        let item = VecItem {
            id: id.to_string(),
            session: session.to_string(),
            model: model.to_string(),
            vector: vector.clone(),
            text: text.to_string(),
            ts,
        };
        {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT OR REPLACE INTO embeddings(id, session, model, dim, vector, text, ts)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    item.id,
                    item.session,
                    item.model,
                    item.vector.len() as i64,
                    encode_f32(&item.vector),
                    item.text,
                    item.ts
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        let bytes = item.bytes();
        {
            let mut l0 = self.l0.lock();
            l0.retain(|x| x.id != item.id);
            l0.push(item);
        }
        self.l0_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.evict_if_needed();
        Ok(json!({"ok": true, "dim": vector.len()}))
    }

    fn vec_search(&self, p: &Value, cold: bool) -> Result<Value, String> {
        let q: Vec<f32> = p
            .get("vector")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_f64().map(|f| f as f32))
                    .collect()
            })
            .unwrap_or_default();
        if q.is_empty() {
            return Ok(json!({"items": []}));
        }
        let k = p.get("k").and_then(|x| x.as_u64()).unwrap_or(5) as usize;
        let model = p
            .get("model")
            .and_then(|x| x.as_str())
            .map(|s| s.trim_matches('"').to_string());
        let mut scored: Vec<(f32, String, String, String, i64)> = Vec::new();

        // L0 热层
        {
            let l0 = self.l0.lock();
            for it in l0.iter() {
                if it.vector.len() != q.len() {
                    continue;
                }
                if let Some(m) = &model {
                    if &it.model != m {
                        continue;
                    }
                }
                scored.push((
                    cosine(&q, &it.vector),
                    it.id.clone(),
                    it.session.clone(),
                    it.text.clone(),
                    it.ts,
                ));
            }
        }

        // L2 冷层（显式全库扫描；受内存预算约束，不常驻）
        if cold && scored.len() < k {
            let conn = self.conn.lock();
            let rows: Vec<(String, String, Vec<u8>, String, i64, String)> = if let Some(m) = &model
            {
                let mut stmt = conn
                    .prepare("SELECT id, session, vector, text, ts, model FROM embeddings WHERE model=?1 AND dim=?2 LIMIT 20000")
                    .map_err(|e| e.to_string())?;
                let it = stmt
                    .query_map(params![m, q.len() as i64], |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                        ))
                    })
                    .map_err(|e| e.to_string())?
                    .filter_map(|x| x.ok())
                    .collect();
                it
            } else {
                let mut stmt = conn
                    .prepare("SELECT id, session, vector, text, ts, model FROM embeddings WHERE dim=?1 LIMIT 20000")
                    .map_err(|e| e.to_string())?;
                let it = stmt
                    .query_map(params![q.len() as i64], |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                        ))
                    })
                    .map_err(|e| e.to_string())?
                    .filter_map(|x| x.ok())
                    .collect();
                it
            };
            for (id, session, blob, text, ts, _m) in rows {
                if scored.iter().any(|(_, i, _, _, _)| i == &id) {
                    continue;
                }
                let v = decode_f32(&blob);
                if v.len() != q.len() {
                    continue;
                }
                scored.push((cosine(&q, &v), id, session, text, ts));
            }
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let items: Vec<Value> = scored
            .into_iter()
            .take(k)
            .map(|(s, id, session, text, ts)| json!({"id": id, "session": session, "text": text, "score": s, "ts": ts}))
            .collect();
        Ok(json!({"items": items, "l0": self.l0.lock().len(), "budget": self.budget}))
    }

    fn stats(&self) -> Result<Value, String> {
        let conn = self.conn.lock();
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap_or(0);
        let embeds: i64 = conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
            .unwrap_or(0);
        let sessions: i64 = conn
            .query_row("SELECT COUNT(DISTINCT session) FROM events", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        drop(conn);
        let db_bytes = std::fs::metadata(&self.db_path)
            .map(|m| m.len())
            .unwrap_or(0);
        Ok(json!({
            "available": true,
            "events": events,
            "embeddings": embeds,
            "sessions": sessions,
            "db_bytes": db_bytes,
            "l0_items": self.l0.lock().len(),
            "l0_bytes": self.l0_bytes.load(Ordering::Relaxed),
            "budget_bytes": self.budget,
            "evictions": self.evictions.load(Ordering::Relaxed),
            "path": self.db_path.display().to_string(),
        }))
    }

    fn gc(&self, dry: bool) -> Result<Value, String> {
        let cutoff =
            (featheragent::proto::types::now_wall_ns() as i64) - 90 * 24 * 3600 * 1_000_000_000i64;
        let conn = self.conn.lock();
        let old_events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE ts < ?1",
                params![cutoff],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let old_embeds: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM embeddings WHERE ts < ?1",
                params![cutoff],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let mut deleted = 0i64;
        if !dry {
            let keep_sessions: i64 = 200;
            let n_sessions: i64 = conn
                .query_row("SELECT COUNT(DISTINCT session) FROM events", [], |r| {
                    r.get(0)
                })
                .unwrap_or(0);
            if n_sessions > keep_sessions {
                let excess = n_sessions - keep_sessions;
                let sessions: Vec<String> = {
                    let mut stmt = conn
                        .prepare("SELECT session, MAX(ts) AS m FROM events GROUP BY session ORDER BY m ASC LIMIT ?1")
                        .map_err(|e| e.to_string())?;
                    let it = stmt
                        .query_map(params![excess], |r| r.get::<_, String>(0))
                        .map_err(|e| e.to_string())?;
                    it.filter_map(|x| x.ok()).collect()
                };
                for s in &sessions {
                    let fts_ids: Vec<i64> = {
                        let mut stmt = conn
                            .prepare("SELECT id FROM events WHERE session=?1")
                            .map_err(|e| e.to_string())?;
                        let it = stmt
                            .query_map(params![s], |r| r.get::<_, i64>(0))
                            .map_err(|e| e.to_string())?;
                        it.filter_map(|x| x.ok()).collect()
                    };
                    for id in fts_ids {
                        let _ = conn.execute("DELETE FROM events_fts WHERE rowid=?1", params![id]);
                    }
                    deleted += conn
                        .execute("DELETE FROM events WHERE session=?1", params![s])
                        .unwrap_or(0) as i64;
                    deleted += conn
                        .execute("DELETE FROM embeddings WHERE session=?1", params![s])
                        .unwrap_or(0) as i64;
                }
            }
        }
        Ok(json!({
            "dry_run": dry,
            "old_events": old_events,
            "old_embeddings": old_embeds,
            "deleted_rows": deleted,
            "retention_days": 90,
        }))
    }
}

fn extract_text(payload: &Value) -> String {
    if let Some(t) = payload.get("text").and_then(|x| x.as_str()) {
        return t.to_string();
    }
    if let Some(r) = payload.get("result") {
        return match r {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    }
    if let Some(e) = payload.get("error").and_then(|x| x.as_str()) {
        return e.to_string();
    }
    String::new()
}

fn encode_f32(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn decode_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0f32;
    let mut na = 0f32;
    let mut nb = 0f32;
    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

fn main() {
    let codec = if std::env::var("FEATHER_CODEC").ok().as_deref() == Some("json") {
        Codec::Json
    } else {
        Codec::MsgPack
    };
    let data = std::env::var("FEATHER_DATA").unwrap_or_else(|_| "/tmp/featheragent-data".into());
    let storage = Storage::open(Path::new(&data));

    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    let mut out = std::io::stdout();
    loop {
        let req = match read_frame(&mut lock, codec) {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(e) => {
                eprintln!("storage read error: {e}");
                break;
            }
        };
        let id = req.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let params = req.get("params").cloned().unwrap_or(Value::Null);

        if method == "$/cancel" {
            continue;
        }

        let result: Result<Value, String> = match method.as_str() {
            "initialize" => Ok(serde_json::to_value(manifest()).unwrap()),
            _ => match &storage {
                Err(e) => Err(e.clone()),
                Ok(st) => match method.as_str() {
                    "index.append" => {
                        let events = params
                            .get("events")
                            .and_then(|x| x.as_array())
                            .cloned()
                            .unwrap_or_default();
                        st.index_append(&events)
                    }
                    "search.fts" => {
                        let q = params.get("q").and_then(|x| x.as_str()).unwrap_or("");
                        let limit =
                            params.get("limit").and_then(|x| x.as_u64()).unwrap_or(5) as usize;
                        st.search_fts(q, limit)
                    }
                    "vec.upsert" => st.vec_upsert(&params),
                    "vec.search" => {
                        let cold = params.get("cold").and_then(|x| x.as_bool()).unwrap_or(true);
                        st.vec_search(&params, cold)
                    }
                    "stats" => st.stats(),
                    "gc" => {
                        let dry = params
                            .get("dry_run")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true);
                        st.gc(dry)
                    }
                    other => Err(format!("未知方法 {other}")),
                },
            },
        };

        let resp = match result {
            Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": e}}),
        };
        if write_frame(&mut out, &resp, codec).is_err() {
            break;
        }
    }
}

fn manifest() -> PluginManifest {
    PluginManifest {
        name: "storage".into(),
        version: featheragent::VERSION.into(),
        api: ">=1.0 <2.0".into(),
        capabilities: vec![
            "storage.fts".into(),
            "storage.vector".into(),
            "storage.stats".into(),
            "storage.gc".into(),
        ],
        permissions: vec!["fs:data".into()],
        category: "storage".into(),
    }
}
