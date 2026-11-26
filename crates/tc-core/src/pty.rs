//! PTY management: one ConPTY per pane, with a replay buffer that outlives
//! any particular UI attachment.

use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Default replay buffer size per pane.
pub const DEFAULT_REPLAY_BYTES: usize = 4 * 1024 * 1024;

pub type OutputFn = Box<dyn Fn(&[u8]) + Send + Sync>;

/// Receives lifecycle notifications from the PTY manager.
pub trait PaneEvents: Send + Sync {
    fn on_exit(&self, pane_id: &str, exit_code: Option<u32>);
    /// An OSC 7501 program-status payload from the pane's output.
    fn on_program_status(&self, _pane_id: &str, _payload: &str) {}
}

#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub pane_id: String,
    pub cwd: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Inherited variables to drop before applying `env`.
    pub env_remove: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

struct Replay {
    data: VecDeque<u8>,
    cap: usize,
    subscriber: Option<(u64, OutputFn)>,
    /// Bytes ever pushed, so positions survive trimming.
    total: u64,
    /// Size changes as (stream position, cols, rows), oldest first.
    sizes: VecDeque<SizeMark>,
}

impl Replay {
    fn new(cap: usize) -> Self {
        Self {
            data: VecDeque::new(),
            cap,
            subscriber: None,
            total: 0,
            sizes: VecDeque::new(),
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.data.extend(chunk);
        self.total += chunk.len() as u64;
        if self.data.len() > self.cap {
            let excess = self.data.len() - self.cap;
            self.data.drain(..excess);
        }
        // Keep the last size before the buffer start; it is the size the
        // oldest buffered byte was written for.
        let start = self.start();
        while self.sizes.len() > 1 && self.sizes[1].at <= start {
            self.sizes.pop_front();
        }
    }

    fn mark_size(&mut self, cols: u16, rows: u16) {
        let mark = SizeMark { at: self.total, cols, rows };
        // Sizes with no output in between don't matter; keep the last one so
        // a resize animation doesn't pile up marks.
        match self.sizes.back_mut() {
            Some(last) if last.at == mark.at => *last = mark,
            _ => self.sizes.push_back(mark),
        }
    }

    /// Stream position of the oldest buffered byte.
    fn start(&self) -> u64 {
        self.total - self.data.len() as u64
    }
}

/// A terminal size that applies to the output from `at` onwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct SizeMark {
    pub at: u64,
    pub cols: u16,
    pub rows: u16,
}

/// The buffered output of a pane, for troubleshooting display problems.
pub struct Capture {
    pub data: Vec<u8>,
    /// Stream position of `data[0]`.
    pub start: u64,
    pub sizes: Vec<SizeMark>,
}

type SharedWriter = Arc<Mutex<Option<Box<dyn Write + Send>>>>;

struct Pane {
    writer: SharedWriter,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    alive: Arc<AtomicBool>,
}

pub struct PtyManager {
    panes: Mutex<HashMap<String, Arc<Pane>>>,
    /// Replay buffers survive respawns of the same pane.
    replays: Mutex<HashMap<String, Arc<Mutex<Replay>>>>,
    events: Arc<dyn PaneEvents>,
    next_attach: AtomicU64,
    replay_cap: usize,
    #[cfg(windows)]
    job: job::Job,
}

impl PtyManager {
    pub fn new(events: Arc<dyn PaneEvents>) -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            replays: Mutex::new(HashMap::new()),
            events,
            next_attach: AtomicU64::new(1),
            replay_cap: DEFAULT_REPLAY_BYTES,
            #[cfg(windows)]
            job: job::Job::new(),
        }
    }

    pub fn is_alive(&self, pane_id: &str) -> bool {
        self.panes
            .lock()
            .get(pane_id)
            .map(|p| p.alive.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    pub fn alive_panes(&self) -> Vec<String> {
        self.panes
            .lock()
            .iter()
            .filter(|(_, p)| p.alive.load(Ordering::SeqCst))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Spawn a process for the pane. Fails if one is already running.
    pub fn spawn(&self, spec: SpawnSpec) -> Result<(), String> {
        if self.is_alive(&spec.pane_id) {
            return Err(format!("pane {} is already running", spec.pane_id));
        }
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: spec.rows.max(2),
                cols: spec.cols.max(10),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty failed: {e}"))?;

        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        cmd.cwd(&spec.cwd);
        for k in &spec.env_remove {
            cmd.env_remove(k);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn failed: {e}"))?;
        drop(pair.slave);

        #[cfg(windows)]
        if let Some(pid) = child.process_id() {
            self.job.assign(pid);
        }

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("reader failed: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("writer failed: {e}"))?;

        let replay = self.replay(&spec.pane_id);
        replay.lock().mark_size(spec.cols.max(10), spec.rows.max(2));

        let alive = Arc::new(AtomicBool::new(true));
        let pane = Arc::new(Pane {
            writer: Arc::new(Mutex::new(Some(writer))),
            master: Mutex::new(Some(pair.master)),
            killer: Mutex::new(child.clone_killer()),
            alive: alive.clone(),
        });
        self.panes.lock().insert(spec.pane_id.clone(), pane.clone());

        // Reader thread: pump PTY output into the replay buffer and subscriber.
        {
            let replay = replay.clone();
            let writer = pane.writer.clone();
            let events = self.events.clone();
            let pane_id = spec.pane_id.clone();
            std::thread::Builder::new()
                .name(format!("pty-read-{}", spec.pane_id))
                .spawn(move || read_loop(reader, replay, writer, events, pane_id))
                .map_err(|e| e.to_string())?;
        }

        // Waiter thread: observe exit, then close the pseudoconsole so the
        // reader sees EOF.
        {
            let pane_id = spec.pane_id.clone();
            let events = self.events.clone();
            let pane = pane.clone();
            std::thread::Builder::new()
                .name(format!("pty-wait-{}", spec.pane_id))
                .spawn(move || {
                    let code = child.wait().ok().map(|s| s.exit_code());
                    pane.alive.store(false, Ordering::SeqCst);
                    pane.writer.lock().take();
                    pane.master.lock().take();
                    events.on_exit(&pane_id, code);
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn write(&self, pane_id: &str, data: &[u8]) -> Result<(), String> {
        let pane = self.get(pane_id)?;
        let mut w = pane.writer.lock();
        match w.as_mut() {
            Some(w) => {
                w.write_all(data).map_err(|e| e.to_string())?;
                w.flush().map_err(|e| e.to_string())
            }
            None => Err("pane is not running".into()),
        }
    }

    pub fn resize(&self, pane_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let pane = self.get(pane_id)?;
        let m = pane.master.lock();
        let (cols, rows) = (cols.max(10), rows.max(2));
        match m.as_ref() {
            Some(m) => {
                m.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| e.to_string())?;
                self.replay(pane_id).lock().mark_size(cols, rows);
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// A copy of the pane's buffered output and the sizes it was written for.
    pub fn capture(&self, pane_id: &str) -> Option<Capture> {
        let replay = self.replays.lock().get(pane_id)?.clone();
        let r = replay.lock();
        let (a, b) = r.data.as_slices();
        let mut data = Vec::with_capacity(a.len() + b.len());
        data.extend_from_slice(a);
        data.extend_from_slice(b);
        Some(Capture {
            data,
            start: r.start(),
            sizes: r.sizes.iter().copied().collect(),
        })
    }

    fn replay(&self, pane_id: &str) -> Arc<Mutex<Replay>> {
        self.replays
            .lock()
            .entry(pane_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(Replay::new(self.replay_cap))))
            .clone()
    }

    pub fn kill(&self, pane_id: &str) {
        if let Some(p) = self.panes.lock().get(pane_id) {
            let _ = p.killer.lock().kill();
        }
    }

    pub fn kill_all(&self) {
        for p in self.panes.lock().values() {
            let _ = p.killer.lock().kill();
        }
    }

    /// Drop the pane's process and its replay buffer entirely.
    pub fn forget(&self, pane_id: &str) {
        self.kill(pane_id);
        self.panes.lock().remove(pane_id);
        self.replays.lock().remove(pane_id);
    }

    /// Attach an output subscriber. The current replay buffer is delivered to
    /// `out` first, then live output, with no gap or duplication. Replaces any
    /// previous subscriber. Returns an attach id for `detach` and the number
    /// of replayed bytes.
    pub fn attach(&self, pane_id: &str, out: OutputFn) -> (u64, usize) {
        let replay = self.replay(pane_id);
        let id = self.next_attach.fetch_add(1, Ordering::SeqCst);
        let mut r = replay.lock();
        let replayed = r.data.len();
        if !r.data.is_empty() {
            let (a, b) = r.data.as_slices();
            let mut snapshot = Vec::with_capacity(a.len() + b.len());
            snapshot.extend_from_slice(a);
            snapshot.extend_from_slice(b);
            out(&snapshot);
        }
        r.subscriber = Some((id, out));
        (id, replayed)
    }

    pub fn detach(&self, pane_id: &str, attach_id: u64) {
        if let Some(replay) = self.replays.lock().get(pane_id) {
            let mut r = replay.lock();
            if matches!(r.subscriber, Some((id, _)) if id == attach_id) {
                r.subscriber = None;
            }
        }
    }

    fn get(&self, pane_id: &str) -> Result<Arc<Pane>, String> {
        self.panes
            .lock()
            .get(pane_id)
            .cloned()
            .ok_or_else(|| format!("unknown pane {pane_id}"))
    }
}

const DSR: &[u8] = b"[6n";

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    replay: Arc<Mutex<Replay>>,
    writer: SharedWriter,
    events: Arc<dyn PaneEvents>,
    pane_id: String,
) {
    let mut buf = vec![0u8; 64 * 1024];
    let mut osc = crate::osc::OscScanner::default();
    // ConPTY (with PSEUDOCONSOLE_INHERIT_CURSOR) asks for the cursor position
    // once at startup and blocks the child until it gets an answer. Answer it
    // here so a pane can start before any UI is attached, and keep the query
    // out of the replay buffer so a later replay doesn't answer it again.
    let mut startup_dsr_pending = true;
    let mut seen = 0usize;
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut chunk: &[u8] = &buf[..n];
                let owned;
                if startup_dsr_pending {
                    if let Some(pos) = chunk.windows(DSR.len()).position(|w| w == DSR) {
                        startup_dsr_pending = false;
                        if let Some(w) = writer.lock().as_mut() {
                            let _ = w.write_all(b"[1;1R");
                            let _ = w.flush();
                        }
                        let mut v = chunk[..pos].to_vec();
                        v.extend_from_slice(&chunk[pos + DSR.len()..]);
                        owned = v;
                        chunk = &owned;
                    } else {
                        seen += n;
                        if seen > 16 * 1024 {
                            startup_dsr_pending = false;
                        }
                    }
                }
                if chunk.is_empty() {
                    continue;
                }
                for payload in osc.feed(chunk) {
                    events.on_program_status(&pane_id, &payload);
                }
                let mut r = replay.lock();
                r.push(chunk);
                if let Some((_, out)) = r.subscriber.as_ref() {
                    out(chunk);
                }
            }
        }
    }
}

#[cfg(windows)]
mod job {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// A job object that kills every assigned process when the app exits.
    pub struct Job(HANDLE);
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> Self {
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if !h.is_null() {
                    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                    SetInformationJobObject(
                        h,
                        JobObjectExtendedLimitInformation,
                        &info as *const _ as *const _,
                        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    );
                }
                Job(h)
            }
        }

        pub fn assign(&self, pid: u32) {
            if self.0.is_null() {
                return;
            }
            unsafe {
                let p = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if !p.is_null() {
                    AssignProcessToJobObject(self.0, p);
                    CloseHandle(p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_keeps_the_sizes_that_cover_the_buffer() {
        let mut r = Replay::new(10);
        r.mark_size(80, 24);
        r.push(b"abcdef");
        r.mark_size(100, 30);
        r.push(b"ghij");
        assert_eq!(r.start(), 0);
        assert_eq!(r.sizes.len(), 2);
        // Trims "abcd": the 80x24 mark still covers "ef".
        r.push(b"klmn");
        assert_eq!(r.start(), 4);
        assert_eq!(r.sizes.len(), 2);
        // Trims past position 6: only the 100x30 mark is left.
        r.push(b"opqrst");
        assert_eq!(r.start(), 10);
        assert_eq!(
            r.sizes.iter().copied().collect::<Vec<_>>(),
            vec![SizeMark { at: 6, cols: 100, rows: 30 }]
        );
        assert_eq!(r.data.iter().copied().collect::<Vec<_>>(), b"klmnopqrst");
        // Resizes with no output in between collapse into the last one.
        r.mark_size(90, 30);
        r.mark_size(80, 30);
        assert_eq!(r.sizes.back(), Some(&SizeMark { at: 20, cols: 80, rows: 30 }));
        assert_eq!(r.sizes.len(), 2);
    }
}
