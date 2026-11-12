//! Conversation history search across AI CLIs.
//!
//! Each CLI keeps its own transcripts:
//! - Claude Code: `~/.claude/projects/<folder>/<session id>.jsonl`
//! - Codex CLI:   `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl[.zst]`
//! - Antigravity CLI (agy): `~/.gemini/antigravity-cli/conversation_summaries.db`
//!
//! The raw transcripts are gigabytes, mostly tool output. We keep an index in
//! SQLite with only what a person would search for (the user's and the
//! assistant's text, the title and the folder) and refresh it incrementally
//! by file size and modification time.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Text kept per conversation; enough to find it, bounded for size.
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
    Agy,
}

impl Agent {
    fn as_str(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Agy => "agy",
        }
    }

    pub fn parse(s: &str) -> Option<Agent> {
        match s {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            "agy" => Some(Agent::Agy),
            _ => None,
        }
    }

    /// The command that resumes conversation `id` in a shell. `id` must have
    /// passed `is_safe_id`, since the line is typed into a shell.
    pub fn resume_line(self, id: &str) -> String {
        match self {
            Agent::Claude => format!("claude --resume {id}\r"),
            Agent::Codex => format!("codex resume {id}\r"),
            Agent::Agy => format!("agy --conversation {id}\r"),
        }
    }
}

/// Conversation ids are UUID-like; anything else is refused because the id
/// ends up on a shell command line.
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub agent: Agent,
    pub id: String,
    pub cwd: String,
    pub title: String,
    /// Unix millis of the last activity.
    pub updated: i64,
    /// Text around the first match of the first search term.
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub indexing: bool,
    pub done: usize,
    pub total: usize,
    pub conversations: usize,
}

/// Where each CLI keeps its data. Defaults follow each CLI's conventions.
#[derive(Debug, Clone)]
pub struct Sources {
    pub claude_projects: Option<PathBuf>,
    pub codex_sessions: Option<PathBuf>,
    pub codex_index: Option<PathBuf>,
    pub agy_summaries: Option<PathBuf>,
}

impl Sources {
    pub fn detect() -> Self {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);
        let claude = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".claude")));
        let codex = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".codex")));
        Sources {
            claude_projects: claude.map(|d| d.join("projects")),
            codex_sessions: codex.as_ref().map(|d| d.join("sessions")),
            codex_index: codex.map(|d| d.join("session_index.jsonl")),
            agy_summaries: home.map(|h| {
                h.join(".gemini")
                    .join("antigravity-cli")
                    .join("conversation_summaries.db")
            }),
        }
    }
}

pub struct HistoryIndex {
    db: Mutex<Connection>,
    sources: Sources,
    progress: Mutex<Progress>,
    running: AtomicBool,
}

struct Parsed {
    id: String,
    cwd: String,
    title: String,
    body: String,
    updated: i64,
}

struct FileSource {
    key: String,
    agent: Agent,
    path: PathBuf,
    mtime: i64,
    size: i64,
}

impl HistoryIndex {
    pub fn open(db_path: &Path, sources: Sources) -> Result<Self, String> {
        let db = Connection::open(db_path).map_err(|e| e.to_string())?;
        db.execute_batch(
            // secure_delete overwrites deleted rows, so text from removed
            // transcripts doesn't linger in free pages of the index.
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA secure_delete=ON;
             CREATE TABLE IF NOT EXISTS conv (
               key TEXT PRIMARY KEY,
               agent TEXT NOT NULL,
               id TEXT NOT NULL,
               cwd TEXT NOT NULL,
               title TEXT NOT NULL,
               updated INTEGER NOT NULL,
               mtime INTEGER NOT NULL,
               size INTEGER NOT NULL,
               body TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS conv_updated ON conv(updated DESC);",
        )
        .map_err(|e| e.to_string())?;
        let count = count_rows(&db);
        Ok(Self {
            db: Mutex::new(db),
            sources,
            progress: Mutex::new(Progress {
                conversations: count,
                ..Default::default()
            }),
            running: AtomicBool::new(false),
        })
    }

    /// Deletes every indexed conversation and shrinks the file.
    pub fn clear(&self) -> Result<(), String> {
        let db = self.db.lock().unwrap();
        db.execute_batch("DELETE FROM conv; VACUUM;")
            .map_err(|e| e.to_string())?;
        self.progress.lock().unwrap().conversations = 0;
        Ok(())
    }

    pub fn progress(&self) -> Progress {
        *self.progress.lock().unwrap()
    }

    /// Brings the index up to date. Returns false without doing anything if
    /// a refresh is already running. `on_progress` is called as files are read.
    pub fn refresh(&self, on_progress: &dyn Fn(Progress)) -> bool {
        if self.running.swap(true, Ordering::SeqCst) {
            return false;
        }
        let result = self.refresh_inner(on_progress);
        let count = count_rows(&self.db.lock().unwrap());
        {
            let mut p = self.progress.lock().unwrap();
            p.indexing = false;
            p.conversations = count;
            on_progress(*p);
        }
        self.running.store(false, Ordering::SeqCst);
        if let Err(e) = result {
            log::warn!("history refresh failed: {e}");
        }
        true
    }

    fn refresh_inner(&self, on_progress: &dyn Fn(Progress)) -> Result<(), String> {
        let mut files = Vec::new();
        if let Some(dir) = &self.sources.claude_projects {
            collect_claude(dir, &mut files);
        }
        if let Some(dir) = &self.sources.codex_sessions {
            collect_codex(dir, &mut files);
        }

        let known: HashMap<String, (i64, i64)> = {
            let db = self.db.lock().unwrap();
            let mut stmt = db
                .prepare("SELECT key, mtime, size FROM conv WHERE agent != 'agy'")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))
                .map_err(|e| e.to_string())?;
            rows.flatten().collect()
        };

        // Forget transcripts that no longer exist.
        let present: HashSet<&str> = files.iter().map(|f| f.key.as_str()).collect();
        let gone: Vec<&String> = known.keys().filter(|k| !present.contains(k.as_str())).collect();
        if !gone.is_empty() {
            let db = self.db.lock().unwrap();
            for k in gone {
                let _ = db.execute("DELETE FROM conv WHERE key = ?1", [k]);
            }
        }

        let stale: Vec<&FileSource> = files
            .iter()
            .filter(|f| known.get(&f.key) != Some(&(f.mtime, f.size)))
            .collect();

        let codex_titles = self
            .sources
            .codex_index
            .as_deref()
            .map(read_codex_titles)
            .unwrap_or_default();

        let total = stale.len() + 1;
        {
            let mut p = self.progress.lock().unwrap();
            *p = Progress {
                indexing: true,
                done: 0,
                total,
                conversations: p.conversations,
            };
            on_progress(*p);
        }

        let mut batch: Vec<(&FileSource, Option<Parsed>)> = Vec::new();
        for (i, f) in stale.iter().enumerate() {
            let parsed = match f.agent {
                Agent::Claude => parse_claude(&f.path),
                Agent::Codex => parse_codex(&f.path, &codex_titles),
                Agent::Agy => None,
            };
            batch.push((f, parsed));
            if batch.len() >= 32 || i + 1 == stale.len() {
                self.write_batch(&batch)?;
                batch.clear();
                let mut p = self.progress.lock().unwrap();
                p.done = i + 1;
                on_progress(*p);
            }
        }

        if let Some(db_path) = &self.sources.agy_summaries {
            if let Err(e) = self.refresh_agy(db_path) {
                log::warn!("agy history: {e}");
            }
        }
        Ok(())
    }

    fn write_batch(&self, batch: &[(&FileSource, Option<Parsed>)]) -> Result<(), String> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for (f, parsed) in batch {
            match parsed {
                Some(p) => {
                    tx.execute(
                        "INSERT OR REPLACE INTO conv (key, agent, id, cwd, title, updated, mtime, size, body)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![f.key, f.agent.as_str(), p.id, p.cwd, p.title, p.updated, f.mtime, f.size, p.body],
                    )
                    .map_err(|e| e.to_string())?;
                }
                // Unreadable or empty: remember the file so it isn't re-read
                // every time, but keep it out of results.
                None => {
                    tx.execute(
                        "INSERT OR REPLACE INTO conv (key, agent, id, cwd, title, updated, mtime, size, body)
                         VALUES (?1, ?2, '', '', '', 0, ?3, ?4, '')",
                        params![f.key, f.agent.as_str(), f.mtime, f.size],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }

    fn refresh_agy(&self, path: &Path) -> Result<(), String> {
        let Ok(meta) = std::fs::metadata(path) else {
            return Ok(());
        };
        let mtime = mtime_ms(&meta);
        let last: Option<i64> = self
            .db
            .lock()
            .unwrap()
            .query_row("SELECT max(mtime) FROM conv WHERE agent = 'agy'", [], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        if last == Some(mtime) {
            return Ok(());
        }
        let src = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        let mut stmt = src
            .prepare(
                "SELECT conversation_id, title, preview, workspace_uris, last_modified_time
                 FROM conversation_summaries WHERE coalesce(nesting_depth, 0) = 0",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, String, String, String, String)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                ))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM conv WHERE agent = 'agy'", [])
            .map_err(|e| e.to_string())?;
        for (id, title, preview, uris, modified) in rows {
            let Some(cwd) = first_file_uri(&uris) else {
                continue;
            };
            let title = if title.trim().is_empty() {
                first_line(&preview)
            } else {
                title
            };
            tx.execute(
                "INSERT OR REPLACE INTO conv (key, agent, id, cwd, title, updated, mtime, size, body)
                 VALUES (?1, 'agy', ?2, ?3, ?4, ?5, ?6, 0, ?7)",
                params![
                    format!("agy:{id}"),
                    id,
                    cwd,
                    title,
                    parse_time_ms(&modified).unwrap_or(mtime),
                    mtime,
                    preview
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// Conversations matching every whitespace-separated term (in the text,
    /// title or folder), newest first. An empty query lists recent ones.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        let mut sql = String::from(
            "SELECT agent, id, cwd, title, updated, \
             CASE WHEN ?1 = '' THEN NULL ELSE \
               substr(body, max(1, instr(lower(body), lower(?1)) - 60), 220) END, \
             instr(lower(body), lower(?1)) \
             FROM conv WHERE id != ''",
        );
        let mut args: Vec<String> = vec![terms.first().cloned().unwrap_or_default()];
        for t in &terms {
            let i = args.len() + 1;
            sql.push_str(&format!(
                " AND (body LIKE ?{i} ESCAPE '\\' OR title LIKE ?{i} ESCAPE '\\' OR cwd LIKE ?{i} ESCAPE '\\')"
            ));
            args.push(format!("%{}%", escape_like(t)));
        }
        sql.push_str(&format!(" ORDER BY updated DESC LIMIT {}", limit.min(500)));

        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                let agent: String = r.get(0)?;
                let matched_in_body: i64 = r.get::<_, Option<i64>>(6)?.unwrap_or(0);
                Ok((
                    agent,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    if matched_in_body > 0 {
                        r.get::<_, Option<String>>(5)?
                    } else {
                        None
                    },
                ))
            })
            .map_err(|e| e.to_string())?;
        Ok(rows
            .flatten()
            .filter_map(|(agent, id, cwd, title, updated, snippet)| {
                Some(Hit {
                    agent: Agent::parse(&agent)?,
                    id,
                    cwd,
                    title,
                    updated,
                    snippet: snippet.map(|s| tidy_snippet(&s)),
                })
            })
            .collect())
    }
}

fn count_rows(db: &Connection) -> usize {
    db.query_row("SELECT count(*) FROM conv WHERE id != ''", [], |r| r.get::<_, i64>(0))
        .map(|n| n as usize)
        .unwrap_or(0)
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn tidy_snippet(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_line(s: &str) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    line.chars().take(120).collect()
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn file_source(key: String, agent: Agent, path: PathBuf) -> Option<FileSource> {
    let meta = std::fs::metadata(&path).ok()?;
    Some(FileSource {
        key,
        agent,
        mtime: mtime_ms(&meta),
        size: meta.len() as i64,
        path,
    })
}

/// Top-level session files only; subagent transcripts live in subfolders.
fn collect_claude(projects: &Path, out: &mut Vec<FileSource>) {
    let Ok(dirs) = std::fs::read_dir(projects) else {
        return;
    };
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_some_and(|e| e == "jsonl") && path.is_file() {
                let key = format!("claude:{}", path.to_string_lossy());
                out.extend(file_source(key, Agent::Claude, path));
            }
        }
    }
}

fn collect_codex(dir: &Path, out: &mut Vec<FileSource>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            collect_codex(&path, out);
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with("rollout-") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")) {
            let key = format!("codex:{}", path.to_string_lossy());
            out.extend(file_source(key, Agent::Codex, path));
        }
    }
}

/// Appends message text to the body, within the size limit.
fn push_body(body: &mut String, text: &str) {
    if body.len() >= MAX_BODY {
        return;
    }
    let room = MAX_BODY - body.len();
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    body.push_str(&text[..end]);
    body.push('\n');
}

/// Injected context (system reminders, command wrappers, environment blocks)
/// starts with a tag; it isn't what anyone would search for.
fn is_injected(text: &str) -> bool {
    text.trim_start().starts_with('<')
}

fn content_texts(content: &Value) -> Vec<&str> {
    match content {
        Value::String(s) => vec![s.as_str()],
        Value::Array(items) => items
            .iter()
            .filter(|i| {
                matches!(
                    i.get("type").and_then(|t| t.as_str()),
                    Some("text") | Some("input_text") | Some("output_text")
                )
            })
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_claude(path: &Path) -> Option<Parsed> {
    let id = path.file_stem()?.to_string_lossy().to_string();
    let meta = std::fs::metadata(path).ok()?;
    let reader = BufReader::new(std::fs::File::open(path).ok()?);
    let mut cwd = String::new();
    let mut ai_title = String::new();
    let mut first_prompt = String::new();
    let mut body = String::new();

    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.contains("\"aiTitle\"") {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if let Some(t) = v.get("aiTitle").and_then(|t| t.as_str()) {
                    ai_title = t.to_string();
                }
            }
            continue;
        }
        let is_user = line.contains("\"type\":\"user\"");
        if !is_user && !line.contains("\"type\":\"assistant\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v.get("isSidechain").and_then(|b| b.as_bool()) == Some(true)
            || v.get("isMeta").and_then(|b| b.as_bool()) == Some(true)
        {
            continue;
        }
        if cwd.is_empty() {
            if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
                cwd = c.to_string();
            }
        }
        let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
            continue;
        };
        for text in content_texts(content) {
            if is_injected(text) {
                continue;
            }
            if is_user && first_prompt.is_empty() {
                first_prompt = first_line(text);
            }
            push_body(&mut body, text);
        }
    }
    if cwd.is_empty() || body.is_empty() {
        return None;
    }
    Some(Parsed {
        id,
        cwd,
        title: if ai_title.is_empty() { first_prompt } else { ai_title },
        body,
        updated: mtime_ms(&meta),
    })
}

fn read_codex_titles(path: &Path) -> HashMap<String, String> {
    let Ok(f) = std::fs::File::open(path) else {
        return HashMap::new();
    };
    let mut titles = HashMap::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let (Some(id), Some(name)) = (
                v.get("id").and_then(|x| x.as_str()),
                v.get("thread_name").and_then(|x| x.as_str()),
            ) {
                titles.insert(id.to_string(), name.to_string());
            }
        }
    }
    titles
}

fn strip_verbatim(p: &str) -> String {
    p.strip_prefix(r"\\?\").unwrap_or(p).to_string()
}

fn parse_codex(path: &Path, titles: &HashMap<String, String>) -> Option<Parsed> {
    let meta = std::fs::metadata(path).ok()?;
    let file = std::fs::File::open(path).ok()?;
    let reader: Box<dyn Read> = if path.extension().is_some_and(|e| e == "zst") {
        Box::new(zstd::stream::read::Decoder::new(file).ok()?)
    } else {
        Box::new(file)
    };
    let mut id = String::new();
    let mut cwd = String::new();
    let mut first_prompt = String::new();
    let mut body = String::new();

    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        if id.is_empty() && line.contains("\"session_meta\"") {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let p = &v["payload"];
            // Subagent threads belong to a parent conversation.
            if p.get("source").and_then(|s| s.get("subagent")).is_some() {
                return None;
            }
            id = p.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
            cwd = strip_verbatim(p.get("cwd").and_then(|x| x.as_str()).unwrap_or_default());
            continue;
        }
        if !line.contains("\"response_item\"") || !line.contains("\"message\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let p = &v["payload"];
        if p.get("type").and_then(|t| t.as_str()) != Some("message") {
            continue;
        }
        let role = p.get("role").and_then(|r| r.as_str()).unwrap_or_default();
        if role != "user" && role != "assistant" {
            continue;
        }
        for text in content_texts(&p["content"]) {
            if is_injected(text) {
                continue;
            }
            if role == "user" && first_prompt.is_empty() {
                first_prompt = first_line(text);
            }
            push_body(&mut body, text);
        }
    }
    if id.is_empty() || cwd.is_empty() || body.is_empty() {
        return None;
    }
    let title = titles.get(&id).cloned().unwrap_or(first_prompt);
    Some(Parsed {
        id,
        cwd,
        title,
        body,
        updated: mtime_ms(&meta),
    })
}

/// The first `file:///C:/...` entry of a JSON array of URIs, as a Windows path.
fn first_file_uri(uris: &str) -> Option<String> {
    let list: Vec<String> = serde_json::from_str(uris).ok()?;
    let uri = list.into_iter().find(|u| u.starts_with("file://"))?;
    let path = uri.trim_start_matches("file://").trim_start_matches('/');
    let decoded = percent_decode(path);
    Some(if cfg!(windows) {
        decoded.replace('/', "\\")
    } else {
        format!("/{decoded}")
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Parses "2026-11-11 04:06:49.7332096+00:00" (UTC) into Unix millis.
fn parse_time_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once([' ', 'T'])?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let time = rest.get(..8)?;
    let mut t = time.split(':').map(|x| x.parse::<i64>());
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 86400) + hh * 3600 + mm * 60 + ss) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tc-history-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn safe_ids() {
        assert!(is_safe_id("019dbe85-7ac2-70a2-9140-869511736004"));
        assert!(!is_safe_id("abc; rm -rf /"));
        assert!(!is_safe_id("a&b"));
        assert!(!is_safe_id(""));
    }

    #[test]
    fn parses_time() {
        assert_eq!(parse_time_ms("1970-01-01 00:00:00+00:00"), Some(0));
        assert_eq!(
            parse_time_ms("2026-11-11 04:06:49.7332096+00:00"),
            Some(1_794_370_009_000)
        );
    }

    #[test]
    fn decodes_file_uris() {
        let p = first_file_uri(r#"["file:///C:/Users/me/my%20app"]"#).unwrap();
        assert!(p.ends_with("my app"), "{p}");
    }

    #[test]
    fn indexes_and_searches_claude_and_codex() {
        let root = tmp("index");
        let claude = root.join("claude").join("C--work-alpha");
        std::fs::create_dir_all(&claude).unwrap();
        let mut f = std::fs::File::create(claude.join("11111111-aaaa.jsonl")).unwrap();
        writeln!(f, r#"{{"type":"user","cwd":"C:\\work\\alpha","message":{{"role":"user","content":"<system-reminder>noise</system-reminder>"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"user","cwd":"C:\\work\\alpha","message":{{"role":"user","content":"ログイン画面のバグを直したい"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","cwd":"C:\\work\\alpha","message":{{"role":"assistant","content":[{{"type":"text","text":"Fixed the OAuth redirect."}},{{"type":"tool_use","name":"Bash","input":{{"command":"secret-tool-output"}}}}]}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"ai-title","aiTitle":"Fix login bug","sessionId":"11111111-aaaa"}}"#).unwrap();
        drop(f);

        let codex = root.join("codex").join("sessions").join("2026").join("11").join("12");
        std::fs::create_dir_all(&codex).unwrap();
        let lines = [
            r#"{"type":"session_meta","payload":{"id":"019d-beef","cwd":"\\\\?\\D:\\beta"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Add a CSV exporter"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done, see export.rs"}]}}"#,
        ]
        .join("\n");
        let compressed = zstd::encode_all(lines.as_bytes(), 3).unwrap();
        std::fs::write(codex.join("rollout-2026-11-12T00-00-00-019d-beef.jsonl.zst"), compressed).unwrap();

        let sources = Sources {
            claude_projects: Some(root.join("claude")),
            codex_sessions: Some(root.join("codex").join("sessions")),
            codex_index: None,
            agy_summaries: None,
        };
        let index = HistoryIndex::open(&root.join("index.db"), sources).unwrap();
        assert!(index.refresh(&|_| {}));
        assert_eq!(index.progress().conversations, 2);

        let hits = index.search("ログイン", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].agent, Agent::Claude);
        assert_eq!(hits[0].title, "Fix login bug");
        assert_eq!(hits[0].cwd, r"C:\work\alpha");
        assert!(hits[0].snippet.as_deref().unwrap().contains("ログイン"));

        let hits = index.search("csv exporter", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].agent, Agent::Codex);
        assert_eq!(hits[0].id, "019d-beef");
        assert_eq!(hits[0].cwd, r"D:\beta");

        // Tool output and injected context are not indexed.
        assert!(index.search("secret-tool-output", 10).unwrap().is_empty());
        assert!(index.search("noise", 10).unwrap().is_empty());
        // Folder names match too; the empty query lists everything.
        assert_eq!(index.search("beta", 10).unwrap().len(), 1);
        assert_eq!(index.search("", 10).unwrap().len(), 2);
        // Literal % and _ don't act as wildcards.
        assert!(index.search("%", 10).unwrap().is_empty());

        // A second refresh reads nothing new and keeps the results.
        assert!(index.refresh(&|_| {}));
        assert_eq!(index.search("", 10).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }
}
