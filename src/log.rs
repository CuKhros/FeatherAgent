//! 事件日志：自研追加格式（设计文档 §3.1）。
//!
//! 记录布局（little-endian，32 字节头）：
//! ```text
//! +0   u32 magic   = 0xFA1B0001
//! +4   u16 kind    事件类型枚举
//! +6   u16 flags   bit0=payload 为 msgpack，bit1=payload 已压缩
//! +8   u64 seq     会话内单调递增
//! +16  u64 ts_wall_ns  墙钟 CLOCK_REALTIME（Unix epoch 纳秒）
//! +24  u32 len     payload 字节数
//! +28  u32 crc32c  覆盖 [0,28) ∪ payload
//! +32  bytes payload
//! ```
//!
//! 段：`data/sessions/<session_id>/<first_seq:016x>.log`，默认 4 MiB 滚动。
//! 旁路索引：`<first_seq:016x>.idx`，每 256 条记录一个 `(seq, offset)` 项（16 B）。
//!
//! 时间戳语义：落库的是**墙钟**；会话内排序权威是 `seq`（勘误 R1-1）。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::proto::types::{now_wall_ns, Event, EventKind};

pub const MAGIC: u32 = 0xFA1B_0001;
pub const HEADER_LEN: usize = 32;
const FLAG_MSGPACK: u16 = 0x0001;
/// 旁路索引项长度：seq u64 + offset u64
const IDX_ENTRY: u64 = 16;
const IDX_STRIDE: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Always,
    Interval,
    Never,
}

impl SyncMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "always" => Some(SyncMode::Always),
            "interval" => Some(SyncMode::Interval),
            "never" => Some(SyncMode::Never),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub segment_size: u64,
    pub sync: SyncMode,
    pub group_commit_ms: u64,
    pub group_commit_bytes: u64,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            segment_size: 4 * 1024 * 1024,
            sync: SyncMode::Interval,
            group_commit_ms: 5,
            group_commit_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
    pub kind: u16,
    pub flags: u16,
    pub seq: u64,
    pub ts_wall_ns: u64,
    pub len: u32,
    pub crc32c: u32,
}

#[derive(Debug)]
pub enum RecordError {
    Short,
    BadMagic,
    BadCrc { expected: u32, actual: u32 },
    UnknownKind(u16),
}

/// 编码一条记录（头 + payload），供写入与测试共用。
pub fn encode_record(
    kind: EventKind,
    seq: u64,
    ts_wall_ns: u64,
    payload: &Value,
    compressed: bool,
) -> io::Result<Vec<u8>> {
    let mut body = rmp_serde::to_vec_named(payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let mut flags = FLAG_MSGPACK;
    if compressed {
        flags |= 0x0002;
    }
    let mut rec = Vec::with_capacity(HEADER_LEN + body.capacity());
    rec.extend_from_slice(&MAGIC.to_le_bytes());
    rec.extend_from_slice(&kind.code().to_le_bytes());
    rec.extend_from_slice(&flags.to_le_bytes());
    rec.extend_from_slice(&seq.to_le_bytes());
    rec.extend_from_slice(&ts_wall_ns.to_le_bytes());
    rec.extend_from_slice(&(body.len() as u32).to_le_bytes());
    rec.extend_from_slice(&0u32.to_le_bytes()); // crc 占位
    let crc = crc32c::crc32c_append(crc32c::crc32c(&rec[0..28]), &body);
    rec[28..32].copy_from_slice(&crc.to_le_bytes());
    rec.append(&mut body);
    Ok(rec)
}

/// 解码一条记录，返回 `(header, payload, total_len)`。
pub fn decode_record(buf: &[u8]) -> Result<(RecordHeader, Value, usize), RecordError> {
    if buf.len() < HEADER_LEN {
        return Err(RecordError::Short);
    }
    let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    if magic != MAGIC {
        return Err(RecordError::BadMagic);
    }
    let kind = u16::from_le_bytes(buf[4..6].try_into().unwrap());
    let flags = u16::from_le_bytes(buf[6..8].try_into().unwrap());
    let seq = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    let ts = u64::from_le_bytes(buf[16..24].try_into().unwrap());
    let len = u32::from_le_bytes(buf[24..28].try_into().unwrap()) as usize;
    let crc = u32::from_le_bytes(buf[28..32].try_into().unwrap());
    let total = HEADER_LEN + len;
    if buf.len() < total {
        return Err(RecordError::Short);
    }
    let payload_bytes = &buf[HEADER_LEN..total];
    let actual = crc32c::crc32c_append(crc32c::crc32c(&buf[0..28]), payload_bytes);
    if actual != crc {
        return Err(RecordError::BadCrc {
            expected: crc,
            actual,
        });
    }
    // 事件类型必须已知；未知类型视为记录损坏（前向兼容通过新增码值解决）
    EventKind::from_code(kind).ok_or(RecordError::UnknownKind(kind))?;
    let payload: Value = rmp_serde::from_slice(payload_bytes).map_err(|_| RecordError::BadMagic)?;
    Ok((
        RecordHeader {
            kind,
            flags,
            seq,
            ts_wall_ns: ts,
            len: len as u32,
            crc32c: crc,
        },
        payload,
        total,
    ))
}

struct Segment {
    #[allow(dead_code)] // 保留给压缩/归档与诊断使用
    path: PathBuf,
    file: File,
    #[allow(dead_code)] // 保留给压缩/归档与诊断使用
    idx_path: PathBuf,
    idx_file: File,
    #[allow(dead_code)] // 段起始序号，诊断与后续 compaction 使用
    first_seq: u64,
    offset: u64,
    records: u32,
}

struct SessionState {
    seg: Option<Segment>,
    next_seq: u64,
    pending_bytes: u64,
    last_sync: Instant,
}

pub struct LogStore {
    root: PathBuf,
    cfg: LogConfig,
    inner: Mutex<HashMap<String, SessionState>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub session: String,
    pub scanned_records: u64,
    pub truncated_bytes: u64,
    pub last_good_seq: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveReport {
    pub session: String,
    pub archive_path: String,
    pub raw_bytes: u64,
    pub compressed_bytes: u64,
    pub segments: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyIssue {
    pub path: String,
    pub offset: u64,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    pub sessions: u64,
    pub records: u64,
    pub issues: Vec<VerifyIssue>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Default)]
struct Scan {
    records: Vec<(u64, u64, Event)>, // (offset, total_len, event)
    last_seq: u64,
    corrupt_at: Option<(u64, String)>,
}

/// 会话 ID 只允许安全字符，防止路径穿越。
pub fn safe_session_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for c in id.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("_default");
    }
    out
}

fn segments_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut segs = Vec::new();
    if !dir.exists() {
        return Ok(segs);
    }
    for entry in fs::read_dir(dir)? {
        let p = entry?.path();
        if p.extension().map(|e| e == "log").unwrap_or(false) {
            segs.push(p);
        }
    }
    segs.sort();
    Ok(segs)
}

fn scan_session(dir: &Path) -> io::Result<Scan> {
    let mut scan = Scan::default();
    for seg in segments_in(dir)? {
        let mut buf = Vec::new();
        File::open(&seg)?.read_to_end(&mut buf)?;
        let mut off = 0usize;
        while off < buf.len() {
            match decode_record(&buf[off..]) {
                Ok((h, payload, total)) => {
                    let kind = EventKind::from_code(h.kind).unwrap();
                    scan.last_seq = scan.last_seq.max(h.seq);
                    scan.records.push((
                        off as u64,
                        total as u64,
                        Event::new(h.seq, h.ts_wall_ns, kind, payload),
                    ));
                    off += total;
                }
                Err(e) => {
                    scan.corrupt_at = Some((off as u64, format!("{e:?}")));
                    break;
                }
            }
        }
        if scan.corrupt_at.is_some() {
            break;
        }
    }
    Ok(scan)
}

fn scan_records(dir: &Path) -> io::Result<Vec<Event>> {
    Ok(scan_session(dir)?
        .records
        .into_iter()
        .map(|(_, _, e)| e)
        .collect())
}

impl LogStore {
    pub fn open(root: &Path, cfg: LogConfig) -> io::Result<Self> {
        fs::create_dir_all(root.join("sessions"))?;
        fs::create_dir_all(root.join("archive"))?;
        Ok(Self {
            root: root.to_path_buf(),
            cfg,
            inner: Mutex::new(HashMap::new()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn session_dir(&self, session: &str) -> PathBuf {
        self.root.join("sessions").join(safe_session_id(session))
    }

    /// 追加一条事件。单写者调用；一次 `write` 系统调用写完整条记录。
    pub fn append(&self, session: &str, kind: EventKind, payload: Value) -> io::Result<Event> {
        self.append_ts(session, kind, payload, now_wall_ns())
    }

    pub fn append_ts(
        &self,
        session: &str,
        kind: EventKind,
        payload: Value,
        ts_wall_ns: u64,
    ) -> io::Result<Event> {
        let dir = self.session_dir(session);
        fs::create_dir_all(&dir)?;
        let mut inner = self.inner.lock().unwrap();
        let st = inner
            .entry(session.to_string())
            .or_insert_with(|| SessionState {
                seg: None,
                next_seq: 1,
                pending_bytes: 0,
                last_sync: Instant::now(),
            });

        // 首次使用时，从已有段恢复 next_seq
        if st.seg.is_none() && st.next_seq == 1 {
            let scan = scan_session(&dir)?;
            st.next_seq = scan.last_seq + 1;
        }

        let seq = st.next_seq;
        let rec = encode_record(kind, seq, ts_wall_ns, &payload, false)?;
        let rec_len = rec.len() as u64;

        let need_rotate = match &st.seg {
            None => true,
            Some(s) => s.offset + rec_len > self.cfg.segment_size,
        };
        if need_rotate {
            st.seg = Some(Self::open_segment(&dir, seq)?);
        }
        let seg = st.seg.as_mut().unwrap();
        if seg.records % IDX_STRIDE == 0 {
            let mut e = [0u8; IDX_ENTRY as usize];
            e[0..8].copy_from_slice(&seq.to_le_bytes());
            e[8..16].copy_from_slice(&seg.offset.to_le_bytes());
            seg.idx_file.write_all(&e)?;
        }
        seg.file.write_all(&rec)?;
        seg.offset += rec_len;
        seg.records += 1;
        st.next_seq = seq + 1;
        st.pending_bytes += rec_len;

        if self.cfg.sync == SyncMode::Always || st.pending_bytes >= self.cfg.group_commit_bytes {
            let Some(seg) = st.seg.as_mut() else {
                unreachable!()
            };
            seg.idx_file.flush()?;
            seg.file.sync_data()?;
            st.pending_bytes = 0;
            st.last_sync = Instant::now();
        }

        Ok(Event::new(seq, ts_wall_ns, kind, payload))
    }

    fn open_segment(dir: &Path, first_seq: u64) -> io::Result<Segment> {
        let base = format!("{first_seq:016x}");
        let path = dir.join(format!("{base}.log"));
        let idx_path = dir.join(format!("{base}.idx"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        let len = file.metadata()?.len();
        let idx_file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&idx_path)?;
        Ok(Segment {
            path,
            file,
            idx_path,
            idx_file,
            first_seq,
            offset: len,
            records: 0,
        })
    }

    /// 组提交：按 `group_commit_ms` 周期调用（内核有独立 flusher 线程）。
    pub fn flush(&self) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        for st in inner.values_mut() {
            let Some(seg) = st.seg.as_mut() else { continue };
            if self.cfg.sync == SyncMode::Never {
                continue;
            }
            let due =
                now.duration_since(st.last_sync) >= Duration::from_millis(self.cfg.group_commit_ms);
            if st.pending_bytes > 0 && due {
                seg.idx_file.flush()?;
                seg.file.sync_data()?;
                st.pending_bytes = 0;
                st.last_sync = now;
            }
        }
        Ok(())
    }

    pub fn read_session(&self, session: &str) -> io::Result<Vec<Event>> {
        scan_records(&self.session_dir(session))
    }

    pub fn read_from(&self, session: &str, from_seq: u64) -> io::Result<Vec<Event>> {
        Ok(self
            .read_session(session)?
            .into_iter()
            .filter(|e| e.seq >= from_seq)
            .collect())
    }

    pub fn list_sessions(&self) -> io::Result<Vec<(String, u64, u64)>> {
        let dir = self.root.join("sessions");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let p = entry?.path();
            if !p.is_dir() {
                continue;
            }
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let evs = scan_records(&p).unwrap_or_default();
            let last_ts = evs.last().map(|e| e.ts_wall_ns).unwrap_or(0);
            out.push((name, evs.len() as u64, last_ts));
        }
        out.sort_by(|a, b| b.2.cmp(&a.2));
        Ok(out)
    }

    /// 崩溃恢复：扫描最后一段，从最后一个 `.idx` 检查点向后校验 CRC，
    /// 损坏则截断到上一条完好记录并写 `log.recovered`。
    pub fn recover(&self, session: &str) -> io::Result<RecoveryReport> {
        let dir = self.session_dir(session);
        let segs = segments_in(&dir)?;
        let Some(last) = segs.last().cloned() else {
            return Ok(RecoveryReport {
                session: session.into(),
                scanned_records: 0,
                truncated_bytes: 0,
                last_good_seq: 0,
                truncated: false,
            });
        };

        // 从最后一个 idx 检查点起扫（无 idx 则从 0）
        let idx_path = last.with_extension("idx");
        let start = if idx_path.exists() {
            let data = fs::read(&idx_path)?;
            if data.len() >= 16 {
                u64::from_le_bytes(data[data.len() - 16 + 8..].try_into().unwrap())
            } else {
                0
            }
        } else {
            0
        };

        let mut buf = Vec::new();
        File::open(&last)?.read_to_end(&mut buf)?;
        let file_len = buf.len() as u64;
        let mut off = start as usize;
        let mut last_good = start as u64;
        let mut last_seq = 0u64;
        let mut corrupt = None;
        let mut scanned: u64 = 0;
        while (off as u64) < file_len {
            match decode_record(&buf[off..]) {
                Ok((h, _, total)) => {
                    last_seq = h.seq;
                    last_good = (off + total) as u64;
                    off += total;
                    scanned += 1;
                }
                Err(e) => {
                    corrupt = Some(format!("{e:?}"));
                    break;
                }
            }
        }

        let truncated_bytes = file_len - last_good;
        let truncated = corrupt.is_some() && truncated_bytes > 0;
        if truncated {
            let f = OpenOptions::new().write(true).open(&last)?;
            f.set_len(last_good)?;
            f.sync_data()?;
            // idx 截断到 <= last_good 的条目，保持旁路索引一致
            Self::truncate_idx(&idx_path, last_good)?;
        }

        let report = RecoveryReport {
            session: session.into(),
            scanned_records: scanned,
            truncated_bytes,
            last_good_seq: last_seq,
            truncated,
        };
        if truncated {
            // 丢掉缓存中的段句柄，再写恢复元事件（会新开一段）
            self.inner.lock().unwrap().remove(session);
            self.append(
                session,
                EventKind::LogRecovered,
                serde_json::json!({
                    "lost_bytes": truncated_bytes,
                    "last_good_seq": last_seq,
                    "detail": corrupt,
                }),
            )?;
        }
        Ok(report)
    }

    fn truncate_idx(idx_path: &Path, limit: u64) -> io::Result<()> {
        if !idx_path.exists() {
            return Ok(());
        }
        let data = fs::read(idx_path)?;
        let mut keep = 0usize;
        let mut i = 0usize;
        while i + 16 <= data.len() {
            let off = u64::from_le_bytes(data[i + 8..i + 16].try_into().unwrap());
            if off > limit {
                break;
            }
            keep = i + 16;
            i += 16;
        }
        if keep != data.len() {
            let f = OpenOptions::new().write(true).open(idx_path)?;
            f.set_len(keep as u64)?;
            f.sync_data()?;
        }
        Ok(())
    }

    /// 归档：段文件顺序拼接 + zstd 压缩为 `archive/<session>.fadb`，
    /// 删除原段并留下 `session.archived` 审计事件。
    pub fn archive(&self, session: &str) -> io::Result<ArchiveReport> {
        let dir = self.session_dir(session);
        let segs = segments_in(&dir)?;
        if segs.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no segments"));
        }
        let mut raw = Vec::new();
        for s in &segs {
            let mut b = Vec::new();
            File::open(s)?.read_to_end(&mut b)?;
            raw.extend_from_slice(&b);
        }
        // 落盘前先校验，避免把损坏数据归档成"干净"文件
        let mut off = 0usize;
        while off < raw.len() {
            match decode_record(&raw[off..]) {
                Ok((_, _, total)) => off += total,
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("verify before archive failed at {off}: {e:?}"),
                    ))
                }
            }
        }
        let compressed = zstd::encode_all(&raw[..], 3)?;
        let archive_dir = self.root.join("archive");
        fs::create_dir_all(&archive_dir)?;
        let out = archive_dir.join(format!("{}.fadb", safe_session_id(session)));
        let tmp = out.with_extension("fadb.tmp");
        fs::write(&tmp, &compressed)?;
        crate::util::replace_file(&tmp, &out)?;

        self.inner.lock().unwrap().remove(session);
        for s in &segs {
            let _ = fs::remove_file(s);
            let _ = fs::remove_file(s.with_extension("idx"));
        }
        let report = ArchiveReport {
            session: session.into(),
            archive_path: out.to_string_lossy().into_owned(),
            raw_bytes: raw.len() as u64,
            compressed_bytes: compressed.len() as u64,
            segments: segs.len() as u64,
        };
        self.append(
            session,
            EventKind::SessionArchived,
            serde_json::json!({
                "archive": report.archive_path,
                "raw_bytes": report.raw_bytes,
                "compressed_bytes": report.compressed_bytes,
            }),
        )?;
        Ok(report)
    }

    /// 无损重导入归档文件到新会话。
    pub fn import_archive(&self, session: &str, fadb: &Path) -> io::Result<u64> {
        let comp = fs::read(fadb)?;
        let raw = zstd::decode_all(&comp[..])?;
        let mut off = 0usize;
        let mut n = 0u64;
        let dir = self.session_dir(session);
        fs::create_dir_all(&dir)?;
        // 直接按原字节复制进一个段文件，保留 seq/ts
        let first_seq = decode_record(&raw).map(|(h, _, _)| h.seq).unwrap_or(1);
        let path = dir.join(format!("{first_seq:016x}.log"));
        let mut f = File::create(&path)?;
        while off < raw.len() {
            match decode_record(&raw[off..]) {
                Ok((_, _, total)) => {
                    f.write_all(&raw[off..off + total])?;
                    off += total;
                    n += 1;
                }
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("corrupt archive at {off}: {e:?}"),
                    ))
                }
            }
        }
        f.sync_data()?;
        Ok(n)
    }

    /// fork：复制事件流为新会话（默认不重放副作用工具，见 §6.4）。
    pub fn fork(&self, parent: &str, child: &str) -> io::Result<u64> {
        let src = self.session_dir(parent);
        let dst = self.session_dir(child);
        if dst.exists() && !segments_in(&dst)?.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target session already has log",
            ));
        }
        fs::create_dir_all(&dst)?;
        for s in segments_in(&src)? {
            let name = s.file_name().unwrap().to_owned();
            fs::copy(&s, dst.join(name))?;
        }
        let scan = scan_session(&dst)?;
        let n = scan.records.len() as u64;
        let last_seq = scan.last_seq;
        // 保证子会话可继续追加：状态表清空，next_seq 由扫描结果推出
        self.inner.lock().unwrap().remove(child);
        self.append(
            child,
            EventKind::SessionForked,
            serde_json::json!({"parent": parent, "copied_events": n, "last_seq": last_seq}),
        )?;
        Ok(n)
    }

    pub fn verify_all(&self) -> io::Result<VerifyReport> {
        let dir = self.root.join("sessions");
        let mut report = VerifyReport {
            sessions: 0,
            records: 0,
            issues: Vec::new(),
        };
        if !dir.exists() {
            return Ok(report);
        }
        for entry in fs::read_dir(&dir)? {
            let p = entry?.path();
            if !p.is_dir() {
                continue;
            }
            report.sessions += 1;
            for seg in segments_in(&p)? {
                let mut buf = Vec::new();
                File::open(&seg)?.read_to_end(&mut buf)?;
                let mut off = 0usize;
                while off < buf.len() {
                    match decode_record(&buf[off..]) {
                        Ok((_, _, total)) => {
                            report.records += 1;
                            off += total;
                        }
                        Err(e) => {
                            report.issues.push(VerifyIssue {
                                path: seg.to_string_lossy().into_owned(),
                                offset: off as u64,
                                error: format!("{e:?}"),
                            });
                            break;
                        }
                    }
                }
            }
        }
        Ok(report)
    }

    /// 会话的段文件与索引文件清单（doctor / 统计用）。
    pub fn segment_files(&self, session: &str) -> io::Result<Vec<PathBuf>> {
        let dir = self.session_dir(session);
        let mut out = segments_in(&dir)?;
        if dir.exists() {
            for e in fs::read_dir(&dir)? {
                let p = e?.path();
                if p.extension().map(|x| x == "idx").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn session_dir_path(&self, session: &str) -> PathBuf {
        self.session_dir(session)
    }
}

impl Drop for LogStore {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// 逐段顺序读取（大日志流式 replay 用）。
pub struct SegmentReader {
    files: Vec<PathBuf>,
    idx: usize,
    buf: Vec<u8>,
    off: usize,
}

impl SegmentReader {
    pub fn open(dir: &Path) -> io::Result<Self> {
        Ok(Self {
            files: segments_in(dir)?,
            idx: 0,
            buf: Vec::new(),
            off: 0,
        })
    }

    pub fn next_event(&mut self) -> io::Result<Option<Event>> {
        loop {
            if self.idx >= self.files.len() {
                return Ok(None);
            }
            if self.off >= self.buf.len() {
                self.buf.clear();
                let mut f = File::open(&self.files[self.idx])?;
                f.seek(SeekFrom::Start(0))?;
                f.read_to_end(&mut self.buf)?;
                self.off = 0;
                self.idx += 1;
                if self.buf.is_empty() {
                    continue;
                }
            }
            match decode_record(&self.buf[self.off..]) {
                Ok((h, payload, total)) => {
                    self.off += total;
                    let kind = EventKind::from_code(h.kind).unwrap();
                    return Ok(Some(Event::new(h.seq, h.ts_wall_ns, kind, payload)));
                }
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{e:?} at segment {:?}",
                            self.files[self.idx.saturating_sub(1)]
                        ),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn record_roundtrip_and_crc() {
        let rec = encode_record(
            EventKind::UserMessage,
            7,
            123456789,
            &json!({"t":"hi"}),
            false,
        )
        .unwrap();
        assert_eq!(
            rec.len(),
            HEADER_LEN + rmp_serde::to_vec_named(&json!({"t":"hi"})).unwrap().len()
        );
        let (h, p, total) = decode_record(&rec).unwrap();
        assert_eq!(h.seq, 7);
        assert_eq!(h.kind, EventKind::UserMessage.code());
        assert_eq!(total, rec.len());
        assert_eq!(p["t"], "hi");

        let mut bad = rec.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert!(matches!(
            decode_record(&bad),
            Err(RecordError::BadCrc { .. })
        ));
    }

    #[test]
    fn append_read_and_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = LogConfig {
            segment_size: 120,
            ..Default::default()
        };
        let store = LogStore::open(dir.path(), cfg).unwrap();
        for i in 0..20 {
            store
                .append(
                    "s1",
                    EventKind::UserMessage,
                    json!({"i": i, "pad": "0123456789"}),
                )
                .unwrap();
        }
        store.flush().unwrap();
        let evs = store.read_session("s1").unwrap();
        assert_eq!(evs.len(), 20);
        assert_eq!(evs[0].seq, 1);
        assert_eq!(evs[19].seq, 20);
        assert!(segments_in(&store.session_dir("s1")).unwrap().len() > 1);
        assert!(store.verify_all().unwrap().ok());
    }

    #[test]
    fn recovery_truncates_to_last_good() {
        let dir = tempfile::tempdir().unwrap();
        let store = LogStore::open(dir.path(), LogConfig::default()).unwrap();
        for i in 0..10 {
            store
                .append("s", EventKind::ModelDelta, json!({"i": i}))
                .unwrap();
        }
        store.flush().unwrap();
        drop(store);
        let seg = segments_in(&dir.path().join("sessions/s"))
            .unwrap()
            .pop()
            .unwrap();
        // 模拟断电：尾部写入半条记录
        let mut data = fs::read(&seg).unwrap();
        let cut = data.len() - 5;
        data.truncate(cut);
        fs::write(&seg, &data).unwrap();
        let store = LogStore::open(dir.path(), LogConfig::default()).unwrap();
        let rep = store.recover("s").unwrap();
        assert!(rep.truncated);
        assert_eq!(store.read_session("s").unwrap().len(), 9 + 1); // 9 条 + log.recovered
        assert!(store.verify_all().unwrap().ok());
    }

    #[test]
    fn archive_and_reimport() {
        let dir = tempfile::tempdir().unwrap();
        let store = LogStore::open(dir.path(), LogConfig::default()).unwrap();
        for i in 0..30 {
            store
                .append("s", EventKind::UserMessage, json!({"i": i}))
                .unwrap();
        }
        store.flush().unwrap();
        let before = store.read_session("s").unwrap().len();
        let rep = store.archive("s").unwrap();
        assert!(Path::new(&rep.archive_path).exists());
        assert!(store.verify_all().unwrap().ok());
        let evs = store.read_session("s").unwrap(); // 只剩 session.archived
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].kind, EventKind::SessionArchived);
        let n = store
            .import_archive("s2", Path::new(&rep.archive_path))
            .unwrap();
        assert_eq!(n as usize, before);
        assert_eq!(store.read_session("s2").unwrap().len(), before);
    }

    #[test]
    fn fork_copies_stream_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let store = LogStore::open(dir.path(), LogConfig::default()).unwrap();
        for i in 0..5 {
            store
                .append("p", EventKind::UserMessage, json!({"i": i}))
                .unwrap();
        }
        let n = store.fork("p", "c").unwrap();
        assert_eq!(n, 5);
        let evs = store.read_session("c").unwrap();
        assert_eq!(evs.len(), 6);
        assert_eq!(evs[5].kind, EventKind::SessionForked);
        store
            .append("c", EventKind::UserMessage, json!({"i": 99}))
            .unwrap();
        assert_eq!(store.read_session("c").unwrap().len(), 7);
    }
}
