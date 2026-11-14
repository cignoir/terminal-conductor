//! Finds image and video files that Claude Code mentions in its replies, for
//! the image panel. Only the assistant's own output counts: text blocks and the path
//! arguments of its tool calls. User messages and tool results are skipped.
//!
//! Network paths are never touched: UNC paths (including `\\?\UNC\` and
//! `file://host/`) and paths on mapped network drives are rejected before
//! any file system call, since opening one would connect (and authenticate)
//! to another machine.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Cursor, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "avif", "svg", // images
    "mp4", "m4v", "webm", "mov", "mkv", "ogv", // videos
];
/// Images are read whole (for thumbnails and the clipboard).
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
/// Videos are served in ranges, never read whole.
pub const MAX_VIDEO_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// The most bytes one range response carries.
pub const MAX_RANGE_BYTES: u64 = 4 * 1024 * 1024;
/// Items kept per pane; the oldest drop off.
pub const MAX_ITEMS: usize = 200;
pub const THUMB_SIZE: u32 = 256;
const THUMB_CACHE_BYTES: usize = 48 * 1024 * 1024;

/// Characters that end a path when scanning backwards from its extension.
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '`' | '(' | ')' | '<' | '>' | '[' | ']' | '{' | '}' | '|' | '*' | '\n' | '\r'
            | '\t' | '「' | '」' | '『' | '』' | '（' | '）' | '【' | '】' | '“' | '”' | '‘' | '’'
    )
}

/// Paths to image files mentioned in `text`, as written (not yet resolved).
pub fn extract_candidates(text: &str) -> Vec<String> {
    // ASCII lowercasing keeps byte offsets.
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    // Every place an image extension ends a word, in text order.
    let mut ends: Vec<(usize, usize)> = Vec::new();
    for ext in EXTENSIONS {
        let pat = format!(".{ext}");
        let mut from = 0;
        while let Some(i) = lower[from..].find(&pat) {
            let dot = from + i;
            let end = dot + pat.len();
            from = end;
            if !bytes
                .get(end)
                .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                ends.push((dot, end));
            }
        }
    }
    ends.sort_unstable();
    let mut out: Vec<String> = Vec::new();
    let mut prev_end = 0;
    for (dot, end) in ends {
        // A path never reaches back past the previous one.
        let seg_start = text[prev_end..dot]
            .char_indices()
            .rev()
            .find(|(_, c)| is_delimiter(*c))
            .map(|(i, c)| prev_end + i + c.len_utf8())
            .unwrap_or(prev_end);
        prev_end = end;
        if let Some(p) = path_in_segment(&text[seg_start..end]) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// Picks the path out of a segment that ends with an image extension.
fn path_in_segment(seg: &str) -> Option<String> {
    // file:// URLs. Only the local form file:///C:/... is accepted.
    if let Some(i) = seg.rfind("file://") {
        let rest = &seg[i + "file://".len()..];
        let rest = rest.strip_prefix('/')?;
        if rest.starts_with('/') {
            return None;
        }
        return Some(percent_decode(rest));
    }
    // The last drive-letter start (C:\ or C:/) not glued to a word before it.
    // Absolute paths may contain spaces, so they run to the extension.
    let b = seg.as_bytes();
    let mut drive = None;
    for i in 0..b.len().saturating_sub(2) {
        if b[i].is_ascii_alphabetic()
            && b[i + 1] == b':'
            && (b[i + 2] == b'\\' || b[i + 2] == b'/')
            && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
        {
            drive = Some(i);
        }
    }
    if let Some(i) = drive {
        let p = &seg[i..];
        // "C:\\" written by JSON-escaping tools or "C://" would still be local,
        // but a scheme like "https://" before it was already excluded above.
        return Some(p.to_string());
    }
    // Otherwise a relative path, home path or /c/... path without spaces.
    let token = seg
        .rsplit(|c: char| c.is_whitespace() || c == '：' || c == '、' || c == '。')
        .next()?
        .trim_start_matches([',', ';', ':', '=']);
    if token.is_empty() || token.contains("://") || token.starts_with('.') && token.len() <= 5 {
        return None;
    }
    Some(token.to_string())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len())
            .then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok())
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(v) = hex {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// True for any spelling of a network path.
fn is_network_syntax(p: &str) -> bool {
    p.starts_with("\\\\") || p.starts_with("//") || p.starts_with("\\/") || p.starts_with("/\\")
}

#[cfg(windows)]
fn is_remote_drive(letter: u8) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_NO_ROOT_DIR: u32 = 1;
    let root: Vec<u16> = format!("{}:\\", letter as char).encode_utf16().chain([0]).collect();
    let t = unsafe { GetDriveTypeW(root.as_ptr()) };
    t == DRIVE_REMOTE || t == DRIVE_NO_ROOT_DIR
}

#[cfg(not(windows))]
fn is_remote_drive(_letter: u8) -> bool {
    false
}

/// Builds the absolute path a candidate refers to, without touching the disk.
fn absolute(candidate: &str, cwd: &str) -> Option<PathBuf> {
    let c = candidate.trim();
    if c.is_empty() || is_network_syntax(c) {
        return None;
    }
    let b = c.as_bytes();
    let p = if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
        PathBuf::from(c)
    } else if let Some(rest) = c.strip_prefix("~/").or_else(|| c.strip_prefix("~\\")) {
        PathBuf::from(std::env::var_os("USERPROFILE")?).join(rest)
    } else if b.len() > 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
        // /c/Users/... as written by POSIX shells on Windows.
        PathBuf::from(format!("{}:\\{}", b[1].to_ascii_uppercase() as char, &c[3..]))
    } else if b[0] == b'/' || b[0] == b'\\' {
        return None;
    } else {
        if cwd.is_empty() || is_network_syntax(cwd) {
            return None;
        }
        Path::new(cwd).join(c)
    };
    let s = p.to_string_lossy();
    let sb = s.as_bytes();
    if is_network_syntax(&s) || !(sb.len() > 2 && sb[0].is_ascii_alphabetic() && sb[1] == b':') {
        return None;
    }
    if is_remote_drive(sb[0].to_ascii_uppercase()) {
        return None;
    }
    Some(p)
}

/// Removes the `\\?\` prefix that canonicalize adds; refuses `\\?\UNC\`.
fn local_display_path(canon: &Path) -> Option<PathBuf> {
    let s = canon.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if rest.len() < 3 || rest.to_ascii_uppercase().starts_with("UNC\\") {
            return None;
        }
        return Some(PathBuf::from(rest));
    }
    if is_network_syntax(&s) {
        return None;
    }
    Some(canon.to_path_buf())
}

/// The MIME type of an image or video file, from its first bytes. Matroska
/// is reported as WebM and QuickTime as MP4, the types the webview plays
/// (it then plays whatever codecs it supports inside them).
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF8") {
        Some("image/gif")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else if head.starts_with(b"BM") {
        Some("image/bmp")
    } else if head.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if head.len() >= 12 && &head[4..8] == b"ftyp" && (&head[8..12] == b"avif" || &head[8..12] == b"avis") {
        Some("image/avif")
    } else if head.len() >= 12 && &head[4..8] == b"ftyp" && !matches!(&head[8..12], b"heic" | b"heix" | b"mif1" | b"msf1") {
        Some("video/mp4")
    } else if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some("video/webm")
    } else if head.starts_with(b"OggS") {
        Some("video/ogg")
    } else {
        let text = String::from_utf8_lossy(&head[..head.len().min(1024)]).to_ascii_lowercase();
        let t = text.trim_start_matches('\u{feff}').trim_start();
        let xmlish = t.starts_with("<?xml") || t.starts_with("<svg") || t.starts_with("<!--");
        (xmlish && text.contains("<svg")).then_some("image/svg+xml")
    }
}

fn read_head(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 1024];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

pub fn is_video(mime: &str) -> bool {
    mime.starts_with("video/")
}

/// A mentioned image or video that exists on a local disk.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub path: PathBuf,
    pub size: u64,
    pub modified: u64,
    pub video: bool,
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Resolves a candidate against the folder Claude was working in, and keeps
/// it only if it is an image or video file on a local disk within the limit.
pub fn resolve(candidate: &str, cwd: &str) -> Option<Resolved> {
    let p = absolute(candidate, cwd)?;
    let meta = std::fs::metadata(&p).ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_VIDEO_BYTES {
        return None;
    }
    let canon = std::fs::canonicalize(&p).ok()?;
    let path = local_display_path(&canon)?;
    let video = is_video(sniff(&read_head(&path)?)?);
    if !video && meta.len() > MAX_FILE_BYTES {
        return None;
    }
    Some(Resolved {
        size: meta.len(),
        modified: mtime_ms(&meta),
        path,
        video,
    })
}

/// One image mention from a transcript line.
#[derive(Debug, Clone, PartialEq)]
pub struct Mention {
    pub candidate: String,
    pub cwd: String,
    pub at: u64,
}

fn has_image_ext(lower: &str) -> bool {
    EXTENSIONS.iter().any(|e| lower.contains(&format!(".{e}")))
}

/// Tool-call argument names that hold a file path.
fn is_path_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    k.contains("path") || k == "file" || k == "filename" || k == "output" || k == "image"
}

/// Image mentions in one transcript line (a JSON object).
pub fn mentions_in_line(line: &str) -> Vec<Mention> {
    if !line.contains("\"assistant\"") || !has_image_ext(&line.to_ascii_lowercase()) {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant")
        || v.get("isSidechain").and_then(|s| s.as_bool()) == Some(true)
    {
        return Vec::new();
    }
    let cwd = v.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string();
    let at = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_iso_ms)
        .unwrap_or(0);
    let mut out = Vec::new();
    let mut push = |candidate: String| {
        if !out.iter().any(|m: &Mention| m.candidate == candidate) {
            out.push(Mention { candidate, cwd: cwd.clone(), at });
        }
    };
    let Some(blocks) = v.pointer("/message/content").and_then(|c| c.as_array()) else {
        return out;
    };
    for b in blocks {
        match b.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                    extract_candidates(t).into_iter().for_each(&mut push);
                }
            }
            Some("tool_use") => {
                if let Some(input) = b.get("input").and_then(|i| i.as_object()) {
                    for (k, val) in input {
                        let Some(s) = val.as_str() else { continue };
                        if !is_path_key(k) {
                            continue;
                        }
                        let lower = s.trim().to_ascii_lowercase();
                        if EXTENSIONS.iter().any(|e| lower.ends_with(&format!(".{e}"))) {
                            push(s.trim().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Milliseconds since the epoch for "2026-11-12T08:54:59.301Z".
pub fn parse_iso_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut ms = 0;
    if b.get(19) == Some(&b'.') {
        let frac: String = s[20..].chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        ms = format!("{frac:0<3}").parse::<i64>().ok()?;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let t = ((days * 24 + h) * 60 + mi) * 60 + se;
    (t >= 0).then(|| (t * 1000 + ms) as u64)
}

/// Reads a transcript incrementally. Only complete lines are consumed.
pub struct TranscriptTail {
    pub path: PathBuf,
    offset: u64,
}

impl TranscriptTail {
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    pub fn read_new(&mut self) -> Vec<Mention> {
        let mut out = Vec::new();
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return out;
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            self.offset = 0;
        }
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return out;
        }
        let mut r = BufReader::with_capacity(256 * 1024, f);
        let mut line = Vec::new();
        loop {
            line.clear();
            match r.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if line.last() != Some(&b'\n') {
                        break; // still being written
                    }
                    self.offset += n as u64;
                    out.extend(mentions_in_line(&String::from_utf8_lossy(&line)));
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageItem {
    /// Random id; the only way the UI can name a file.
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    /// File modification time (ms); changes when the file is rewritten.
    pub modified: u64,
    /// When Claude mentioned it (ms).
    pub found_at: u64,
    /// "image" or "video".
    pub kind: &'static str,
}

#[derive(Default)]
struct PaneImages {
    session_id: Option<String>,
    tail: Option<TranscriptTail>,
    items: Vec<ImageItem>,
}

/// Images found per pane, plus the id -> file table the image protocol serves.
#[derive(Default)]
pub struct ImageRegistry {
    panes: HashMap<String, PaneImages>,
    files: HashMap<String, PathBuf>,
    thumbs: ThumbCache,
}

/// Work taken out of the registry so file IO happens without its lock.
pub struct ScanJob {
    pane_id: String,
    session_id: String,
    tail: TranscriptTail,
}

impl ImageRegistry {
    /// Starts a scan of the pane's session transcript; None if the
    /// transcript isn't there (yet). Scans of one registry must run one at a
    /// time (the app uses a single worker thread).
    pub fn begin_scan(&mut self, pane_id: &str, session_id: &str) -> Option<ScanJob> {
        let pane = self.panes.entry(pane_id.to_string()).or_default();
        if pane.session_id.as_deref() != Some(session_id) {
            pane.session_id = Some(session_id.to_string());
            pane.tail = None;
        }
        let tail = match pane.tail.take() {
            Some(t) => t,
            None => TranscriptTail::new(crate::launch::find_transcript(session_id)?),
        };
        Some(ScanJob {
            pane_id: pane_id.to_string(),
            session_id: session_id.to_string(),
            tail,
        })
    }

    /// Merges a finished scan. Returns the pane's items if they changed.
    pub fn finish_scan(&mut self, job: ScanJob, found: Vec<(Mention, Resolved)>) -> Option<Vec<ImageItem>> {
        let ScanJob { pane_id, session_id, tail } = job;
        let pane = self.panes.entry(pane_id).or_default();
        if pane.session_id.as_deref() == Some(session_id.as_str()) {
            pane.tail = Some(tail);
        }
        let mut changed = false;
        for (m, r) in found {
            let path = r.path.to_string_lossy().to_string();
            let key = path.to_lowercase();
            let found_at = if m.at > 0 { m.at } else { crate::store::now_ms() };
            if let Some(i) = pane.items.iter().position(|it| it.path.to_lowercase() == key) {
                let mut it = pane.items.remove(i);
                changed |= i != 0 || it.modified != r.modified;
                it.modified = r.modified;
                it.size = r.size;
                it.found_at = found_at.max(it.found_at);
                it.kind = if r.video { "video" } else { "image" };
                pane.items.insert(0, it);
            } else {
                let id = uuid::Uuid::new_v4().simple().to_string();
                self.files.insert(id.clone(), r.path.clone());
                pane.items.insert(
                    0,
                    ImageItem {
                        id,
                        name: r.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        path,
                        size: r.size,
                        modified: r.modified,
                        found_at,
                        kind: if r.video { "video" } else { "image" },
                    },
                );
                changed = true;
            }
        }
        while pane.items.len() > MAX_ITEMS {
            if let Some(it) = pane.items.pop() {
                self.files.remove(&it.id);
            }
        }
        changed.then(|| pane.items.clone())
    }

    pub fn items(&self, pane_id: &str) -> Vec<ImageItem> {
        self.panes.get(pane_id).map(|p| p.items.clone()).unwrap_or_default()
    }

    pub fn has_session(&self, pane_id: &str) -> bool {
        self.panes.get(pane_id).is_some_and(|p| p.session_id.is_some())
    }

    pub fn remove(&mut self, pane_id: &str, id: &str) -> Vec<ImageItem> {
        if let Some(p) = self.panes.get_mut(pane_id) {
            p.items.retain(|it| it.id != id);
        }
        self.files.remove(id);
        self.items(pane_id)
    }

    pub fn clear(&mut self, pane_id: &str) {
        if let Some(p) = self.panes.get_mut(pane_id) {
            for it in p.items.drain(..) {
                self.files.remove(&it.id);
            }
        }
    }

    pub fn file(&self, id: &str) -> Option<PathBuf> {
        self.files.get(id).cloned()
    }

    pub fn cached_thumb(&mut self, id: &str, modified: u64) -> Option<(Vec<u8>, &'static str)> {
        self.thumbs.get(id, modified)
    }

    pub fn store_thumb(&mut self, id: &str, modified: u64, data: Vec<u8>, mime: &'static str) {
        self.thumbs.put(id, modified, data, mime);
    }
}

impl ScanJob {
    /// Reads the transcript's new lines and resolves the mentions (file IO).
    pub fn run(&mut self) -> Vec<(Mention, Resolved)> {
        self.tail
            .read_new()
            .into_iter()
            .filter_map(|m| resolve(&m.candidate, &m.cwd).map(|r| (m, r)))
            .collect()
    }
}

#[derive(Default)]
struct ThumbCache {
    map: HashMap<(String, u64), (Vec<u8>, &'static str)>,
    order: VecDeque<(String, u64)>,
    bytes: usize,
}

impl ThumbCache {
    fn get(&mut self, id: &str, modified: u64) -> Option<(Vec<u8>, &'static str)> {
        self.map.get(&(id.to_string(), modified)).cloned()
    }

    fn put(&mut self, id: &str, modified: u64, data: Vec<u8>, mime: &'static str) {
        let key = (id.to_string(), modified);
        if data.len() > THUMB_CACHE_BYTES / 4 || self.map.contains_key(&key) {
            return;
        }
        self.bytes += data.len();
        self.map.insert(key.clone(), (data, mime));
        self.order.push_back(key);
        while self.bytes > THUMB_CACHE_BYTES {
            let Some(old) = self.order.pop_front() else { break };
            if let Some((d, _)) = self.map.remove(&old) {
                self.bytes -= d.len();
            }
        }
    }
}

/// Reads an image file for serving, re-checking it is still a local image.
pub fn read_image(path: &Path) -> Option<(Vec<u8>, &'static str, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    let mime = sniff(&data)?;
    (!is_video(mime)).then_some((data, mime, mtime_ms(&meta)))
}

/// One HTTP range of a video file.
pub struct VideoRange {
    pub data: Vec<u8>,
    pub start: u64,
    pub total: u64,
    pub mime: &'static str,
    /// The request asked for a range (answer 206 rather than 200).
    pub partial: bool,
}

/// Parses "bytes=a-b", "bytes=a-" and "bytes=-n" (a single range).
fn parse_range(header: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let spec = header?.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        let n = n.min(total);
        return (n > 0).then(|| (total - n, total - 1));
    }
    let start: u64 = a.parse().ok()?;
    let end = if b.is_empty() { total.saturating_sub(1) } else { b.parse::<u64>().ok()?.min(total.saturating_sub(1)) };
    (start <= end && start < total).then_some((start, end))
}

/// Reads part of a video. Responses are capped at MAX_RANGE_BYTES; the
/// player asks for the rest as it needs it.
pub fn read_video_range(path: &Path, range: Option<&str>) -> Option<VideoRange> {
    use std::io::Read;
    let meta = std::fs::metadata(path).ok()?;
    let total = meta.len();
    if !meta.is_file() || total == 0 || total > MAX_VIDEO_BYTES {
        return None;
    }
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; 64];
    let n = f.read(&mut head).ok()?;
    let mime = sniff(&head[..n]).filter(|m| is_video(m))?;
    let (start, end) = match range {
        Some(_) => parse_range(range, total)?,
        None => (0, total - 1),
    };
    let len = (end - start + 1).min(MAX_RANGE_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut data = vec![0u8; len as usize];
    f.read_exact(&mut data).ok()?;
    Some(VideoRange {
        data,
        start,
        total,
        mime,
        partial: range.is_some() || len < total,
    })
}

fn decode(data: &[u8]) -> Option<image::DynamicImage> {
    let mut reader = image::ImageReader::new(Cursor::new(data)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().ok()
}

/// A small PNG for the panel. Formats the decoder doesn't handle (SVG, AVIF)
/// and images already small are returned as they are.
pub fn thumbnail(data: Vec<u8>, mime: &'static str) -> (Vec<u8>, &'static str) {
    if matches!(mime, "image/svg+xml" | "image/avif") {
        return (data, mime);
    }
    let Some(img) = decode(&data) else {
        return (data, mime);
    };
    if img.width() <= THUMB_SIZE && img.height() <= THUMB_SIZE {
        return (data, mime);
    }
    let t = img.thumbnail(THUMB_SIZE, THUMB_SIZE);
    let mut out = Cursor::new(Vec::new());
    match t.write_to(&mut out, image::ImageFormat::Png) {
        Ok(()) => (out.into_inner(), "image/png"),
        Err(_) => (data, mime),
    }
}

/// RGBA pixels for copying an image to the clipboard.
pub fn decode_rgba(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let (data, _, _) = read_image(path)?;
    let img = decode(&data)?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_absolute_paths_with_spaces_and_japanese() {
        assert_eq!(
            extract_candidates(r"Saved the screenshot to C:\Users\me\My Pictures\shot 1.png."),
            vec![r"C:\Users\me\My Pictures\shot 1.png"]
        );
        assert_eq!(
            extract_candidates("画像はC:\\作業\\画像\\結果.PNGに保存しました"),
            vec!["C:\\作業\\画像\\結果.PNG"]
        );
        assert_eq!(extract_candidates("see `G:/w/a.jpg` and (G:/w/b.webp)"), vec!["G:/w/a.jpg", "G:/w/b.webp"]);
    }

    #[test]
    fn an_absolute_path_does_not_swallow_the_next_one() {
        assert_eq!(
            extract_candidates(r"Saved C:\w\chart one.png and shots/画像2.png"),
            vec![r"C:\w\chart one.png", "shots/画像2.png"]
        );
    }

    #[test]
    fn two_paths_in_one_sentence() {
        assert_eq!(
            extract_candidates(r"compare C:\a\x.png and C:\b\y.png"),
            vec![r"C:\a\x.png", r"C:\b\y.png"]
        );
    }

    #[test]
    fn relative_markdown_home_and_posix_drive_paths() {
        assert_eq!(extract_candidates("![preview](docs/shot.png)"), vec!["docs/shot.png"]);
        assert_eq!(extract_candidates("wrote out/chart.svg and ~/Desktop/a.gif"), vec!["out/chart.svg", "~/Desktop/a.gif"]);
        assert_eq!(extract_candidates("at /g/workspace/x.png"), vec!["/g/workspace/x.png"]);
    }

    #[test]
    fn ignores_non_image_and_word_suffixes() {
        assert!(extract_candidates("a.pngx and b.png_old and .png alone").is_empty());
        assert!(extract_candidates("https://example.com/a.png").is_empty());
    }

    #[test]
    fn file_urls() {
        assert_eq!(extract_candidates("file:///C:/My%20Pics/a.png"), vec!["C:/My Pics/a.png"]);
        assert!(extract_candidates("file://server/share/a.png").is_empty());
    }

    #[test]
    fn network_paths_are_rejected_before_any_io() {
        for p in [
            r"\\server\share\a.png",
            "//server/share/a.png",
            r"\\?\UNC\server\share\a.png",
            r"\\?\C:\a.png",
            r"\\.\C:\a.png",
            r"\/server/a.png",
            r"\a.png",
            "/a.png",
        ] {
            assert!(absolute(p, r"C:\work").is_none(), "{p}");
        }
        assert!(absolute("a.png", r"\\server\share\proj").is_none());
        assert!(local_display_path(Path::new(r"\\?\UNC\server\share\a.png")).is_none());
        assert_eq!(local_display_path(Path::new(r"\\?\C:\a.png")), Some(PathBuf::from(r"C:\a.png")));
        // Text containing a UNC path yields nothing usable.
        for c in extract_candidates(r"look at \\server\share\a.png") {
            assert!(absolute(&c, r"C:\work").is_none(), "{c}");
        }
    }

    #[test]
    fn video_paths_and_types() {
        assert_eq!(extract_candidates(r"recorded C:\w\demo run.mp4 and out/clip.webm"), vec![r"C:\w\demo run.mp4", "out/clip.webm"]);
        assert_eq!(sniff(b"\0\0\0\x20ftypisom\0\0\x02\0"), Some("video/mp4"));
        assert_eq!(sniff(b"\0\0\0\x14ftypqt  \0\0\0\0"), Some("video/mp4"));
        assert_eq!(sniff(b"\0\0\0\x18ftypheic\0\0\0\0"), None);
        assert_eq!(sniff(&[0x1A, 0x45, 0xDF, 0xA3, 0x9F]), Some("video/webm"));
    }

    #[test]
    fn video_ranges() {
        assert_eq!(parse_range(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Some((10, 19)));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Some((90, 99)));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Some((90, 99)));
        assert_eq!(parse_range(Some("bytes=100-"), 100), None);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), None);
        assert_eq!(parse_range(Some("items=0-1"), 100), None);

        let dir = std::env::temp_dir().join(format!("tc-vid-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut bytes = b"\0\0\0\x20ftypisom".to_vec();
        bytes.resize(10_000, 7);
        let v = dir.join("v.mp4");
        std::fs::write(&v, &bytes).unwrap();
        let r = read_video_range(&v, Some("bytes=100-199")).unwrap();
        assert_eq!((r.start, r.total, r.data.len(), r.partial, r.mime), (100, 10_000, 100, true, "video/mp4"));
        let r = read_video_range(&v, None).unwrap();
        assert_eq!((r.data.len(), r.partial), (10_000, false));
        assert!(read_image(&v).is_none(), "videos are never read whole");
        let res = resolve("v.mp4", &dir.to_string_lossy()).unwrap();
        assert!(res.video);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sniffs_formats() {
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0, 0]), Some("image/png"));
        assert_eq!(sniff(b"<?xml version=\"1.0\"?><svg></svg>"), Some("image/svg+xml"));
        assert_eq!(sniff(b"<html></html>"), None);
        assert_eq!(sniff(b"not an image"), None);
    }

    #[test]
    fn only_assistant_output_counts() {
        let user = r#"{"type":"user","cwd":"C:\\w","message":{"content":"see C:\\w\\a.png"}}"#;
        assert!(mentions_in_line(user).is_empty());
        let side = r#"{"type":"assistant","isSidechain":true,"cwd":"C:\\w","message":{"content":[{"type":"text","text":"C:\\w\\a.png"}]}}"#;
        assert!(mentions_in_line(side).is_empty());
        let a = r#"{"type":"assistant","isSidechain":false,"cwd":"C:\\w","timestamp":"2026-11-12T08:54:59.301Z","message":{"content":[
            {"type":"text","text":"Saved to out\\a.png"},
            {"type":"tool_use","name":"Read","input":{"file_path":"C:\\w\\my shot.png"}},
            {"type":"tool_use","name":"Bash","input":{"command":"dir *.png C:\\w\\b.png"}}]}}"#
            .replace('\n', "");
        let m = mentions_in_line(&a);
        let c: Vec<&str> = m.iter().map(|x| x.candidate.as_str()).collect();
        assert_eq!(c, vec!["out\\a.png", "C:\\w\\my shot.png"]);
        assert_eq!(m[0].cwd, "C:\\w");
        assert_eq!(m[0].at, 1_794_473_699_301);
    }

    #[test]
    fn iso_timestamps() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso_ms("2000-03-01T00:00:00.5Z"), Some(951_868_800_500));
    }

    #[test]
    fn resolves_real_files_and_tails_transcripts() {
        let dir = std::env::temp_dir().join(format!("tc-img-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("sub dir")).unwrap();
        let png = dir.join("sub dir").join("画像 1.png");
        let mut buf = Cursor::new(Vec::new());
        image::RgbaImage::new(600, 300).write_to(&mut buf, image::ImageFormat::Png).unwrap();
        std::fs::write(&png, buf.get_ref()).unwrap();
        std::fs::write(dir.join("fake.png"), b"not really").unwrap();
        let cwd = dir.to_string_lossy().to_string();

        let r = resolve("sub dir/画像 1.png", &cwd).expect("relative path resolves");
        assert_eq!(r.path, png);
        assert!(resolve("fake.png", &cwd).is_none(), "content must be an image");
        assert!(resolve("missing.png", &cwd).is_none());

        let (data, mime, _) = read_image(&png).unwrap();
        let (thumb, tmime) = thumbnail(data, mime);
        assert_eq!(tmime, "image/png");
        let t = decode(&thumb).unwrap();
        assert_eq!((t.width(), t.height()), (256, 128));

        let transcript = dir.join("t.jsonl");
        let line = |p: &str| {
            serde_json::json!({"type":"assistant","cwd":cwd,"message":{"content":[{"type":"text","text":format!("done: {p}")}]}})
                .to_string()
        };
        std::fs::write(&transcript, format!("{}\n{}", line(&png.to_string_lossy()), line("partial.png"))).unwrap();
        let mut tail = TranscriptTail::new(transcript.clone());
        assert_eq!(tail.read_new().len(), 1, "the unfinished last line waits");
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(&transcript).unwrap().write_all(b"\n").unwrap();
        assert_eq!(tail.read_new()[0].candidate, "partial.png");
        assert!(tail.read_new().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
