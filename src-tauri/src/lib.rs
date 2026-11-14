mod updater;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};
use tc_core::launch::{
    Launcher, ShellKind, claude_continue_line, claude_start_line, folder_has_transcripts,
    transcript_exists,
};
use tc_core::history::{Agent, HistoryIndex, Hit, Progress, Sources, is_safe_id};
use tc_core::images::{ImageItem, ImageRegistry};
use tc_core::pty::{PaneEvents, PtyManager};
use tc_core::status::{HookMessage, StatusUpdate, interpret};
use tc_core::store::{SessionInfo, SessionStore, load_json, now_ms, save_json};

pub use tc_core::hook::run_hook_client;

struct AppState {
    pty: PtyManager,
    sessions: Arc<Mutex<SessionStore>>,
    launcher: Launcher,
    data_dir: PathBuf,
    /// The conversation search index. Lives in the local (non-roaming)
    /// app data folder and is opened only once the user turns search on.
    history_path: PathBuf,
    history: Mutex<Option<Arc<HistoryIndex>>>,
    /// Images Claude mentioned, per top pane (see tc_core::images).
    images: Arc<Mutex<ImageRegistry>>,
    image_scans: ImageScans,
}

/// Queues transcript scans for the image panel onto one worker thread.
#[derive(Clone)]
struct ImageScans(std::sync::mpsc::Sender<(String, String)>);

impl ImageScans {
    fn start(app: AppHandle, images: Arc<Mutex<ImageRegistry>>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
        let _ = std::thread::Builder::new()
            .name("image-scan".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    // Hooks come in bursts; scan each pane once per burst.
                    let mut batch = vec![first];
                    while let Ok(more) = rx.try_recv() {
                        if !batch.contains(&more) {
                            batch.push(more);
                        }
                    }
                    for (pane_id, session_id) in batch {
                        let Some(mut job) = images.lock().begin_scan(&pane_id, &session_id) else {
                            continue;
                        };
                        let found = job.run();
                        if let Some(items) = images.lock().finish_scan(job, found) {
                            let _ = app.emit("images-changed", ImagesChanged { pane_id, items });
                        }
                    }
                }
            });
        Self(tx)
    }

    /// Only the top pane is watched: the panel shows what the AI there wrote.
    fn request(&self, pane_id: &str, session_id: &str) {
        if pane_id.ends_with(":claude") {
            let _ = self.0.send((pane_id.to_string(), session_id.to_string()));
        }
    }

    /// Scans now and again shortly after: the Stop hook can arrive before
    /// Claude Code has appended its final reply to the transcript.
    fn request_settled(&self, pane_id: &str, session_id: &str) {
        self.request(pane_id, session_id);
        if !pane_id.ends_with(":claude") {
            return;
        }
        let (tx, pane, session) = (self.0.clone(), pane_id.to_string(), session_id.to_string());
        let _ = std::thread::Builder::new().name("image-rescan".into()).spawn(move || {
            for delay in [800, 3000] {
                std::thread::sleep(std::time::Duration::from_millis(delay));
                if tx.send((pane.clone(), session.clone())).is_err() {
                    return;
                }
            }
        });
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImagesChanged {
    pane_id: String,
    items: Vec<ImageItem>,
}

struct EventBridge(AppHandle);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaneExit {
    pane_id: String,
    exit_code: Option<u32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgramStatusEvent {
    pane_id: String,
    payload: String,
}

impl PaneEvents for EventBridge {
    fn on_program_status(&self, pane_id: &str, payload: &str) {
        let _ = self.0.emit(
            "program-status",
            ProgramStatusEvent {
                pane_id: pane_id.to_string(),
                payload: payload.to_string(),
            },
        );
    }

    fn on_exit(&self, pane_id: &str, exit_code: Option<u32>) {
        let _ = self.0.emit(
            "pane-exit",
            PaneExit {
                pane_id: pane_id.to_string(),
                exit_code,
            },
        );
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitData {
    data_dir: String,
    claude_path: Option<String>,
    projects: Value,
    workspace: Value,
    settings: Value,
    sessions: std::collections::HashMap<String, SessionInfo>,
}

fn file(state: &AppState, name: &str) -> PathBuf {
    state.data_dir.join(name)
}

#[tauri::command]
fn app_init(state: State<'_, AppState>) -> InitData {
    InitData {
        data_dir: state.data_dir.to_string_lossy().to_string(),
        claude_path: state
            .launcher
            .claude_exe
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        projects: load_json(&file(&state, "projects.json")).unwrap_or(Value::Null),
        workspace: load_json(&file(&state, "workspace.json")).unwrap_or(Value::Null),
        settings: load_json(&file(&state, "settings.json")).unwrap_or(Value::Null),
        sessions: state.sessions.lock().map.clone(),
    }
}

#[tauri::command]
fn save_doc(state: State<'_, AppState>, name: String, value: Value) -> Result<(), String> {
    if !matches!(name.as_str(), "projects" | "workspace" | "settings") {
        return Err(format!("unknown document {name}"));
    }
    save_json(&file(&state, &format!("{name}.json")), &value)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnsureArgs {
    pane_id: String,
    cwd: String,
    shell: String,
    cols: u16,
    rows: u16,
    /// Start Claude Code in the pane right after the shell, and how.
    #[serde(default)]
    launch: Option<LaunchMode>,
}

/// How to start Claude Code in a pane.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum LaunchMode {
    /// Resume the pane's saved conversation, or start a new one.
    Resume,
    /// Like `Resume`, but on the pane's first launch continue the folder's
    /// latest conversation (e.g. one started in another terminal).
    ResumeOrContinue,
    /// Continue the folder's latest conversation.
    Continue,
    /// Always start a new conversation.
    New,
}

/// Starts the pane's process if it isn't running. Returns true if spawned.
#[tauri::command]
fn pane_ensure(state: State<'_, AppState>, args: EnsureArgs) -> Result<bool, String> {
    if state.pty.is_alive(&args.pane_id) {
        return Ok(false);
    }
    if !std::path::Path::new(&args.cwd).is_dir() {
        return Err(format!("Folder not found: {}", args.cwd));
    }
    let spec = state.launcher.shell_spec(
        &args.pane_id,
        &args.cwd,
        ShellKind::parse(&args.shell),
        args.cols,
        args.rows,
    );
    state.pty.spawn(spec)?;
    if let Some(mode) = args.launch {
        launch_claude(&state, &args.pane_id, &args.cwd, mode)?;
    }
    Ok(true)
}

/// Types the Claude Code command into the pane's shell.
fn launch_claude(state: &AppState, pane_id: &str, cwd: &str, mode: LaunchMode) -> Result<(), String> {
    let mut store = state.sessions.lock();
    let entry = store.map.entry(pane_id.to_string()).or_default();
    let resumable = if mode == LaunchMode::New {
        None
    } else {
        entry.session_id.clone().filter(|id| transcript_exists(id))
    };
    let first_time = entry.session_id.is_none();
    let line = match (mode, resumable) {
        (LaunchMode::Continue, _) => claude_continue_line(),
        (_, Some(id)) => claude_start_line(&id, true),
        // The session id arrives with the SessionStart hook.
        (LaunchMode::ResumeOrContinue, None) if first_time && folder_has_transcripts(cwd) => {
            claude_continue_line()
        }
        _ => {
            let id = uuid::Uuid::new_v4().to_string();
            entry.session_id = Some(id.clone());
            entry.last_assistant_message = None;
            entry.session_title = None;
            claude_start_line(&id, false)
        }
    };
    store.save();
    drop(store);
    state.pty.write(pane_id, line.as_bytes())
}

/// Starts Claude Code in an already running pane (the "Resume" button).
#[tauri::command]
fn claude_launch(
    state: State<'_, AppState>,
    pane_id: String,
    cwd: String,
    mode: LaunchMode,
) -> Result<(), String> {
    launch_claude(&state, &pane_id, &cwd, mode)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResumeInfo {
    /// The pane's saved conversation, if it can be resumed.
    session_id: Option<String>,
    /// Claude Code has conversations for this folder (so --continue works).
    can_continue: bool,
}

/// What a pane could resume, for offering a button instead of auto-starting.
#[tauri::command]
fn claude_resume_info(state: State<'_, AppState>, pane_id: String, cwd: String) -> ResumeInfo {
    let session_id = state
        .sessions
        .lock()
        .map
        .get(&pane_id)
        .and_then(|e| e.session_id.clone())
        .filter(|id| transcript_exists(id));
    ResumeInfo {
        session_id,
        can_continue: folder_has_transcripts(&cwd),
    }
}

#[tauri::command]
fn pane_attach(
    state: State<'_, AppState>,
    pane_id: String,
    channel: Channel<InvokeResponseBody>,
) -> AttachResult {
    let (attach_id, replayed) = state.pty.attach(
        &pane_id,
        Box::new(move |bytes| {
            let _ = channel.send(InvokeResponseBody::Raw(bytes.to_vec()));
        }),
    );
    AttachResult {
        attach_id,
        replayed,
        alive: state.pty.is_alive(&pane_id),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachResult {
    attach_id: u64,
    replayed: usize,
    alive: bool,
}

#[tauri::command]
fn pane_detach(state: State<'_, AppState>, pane_id: String, attach_id: u64) {
    state.pty.detach(&pane_id, attach_id);
}

#[tauri::command]
fn pane_write(state: State<'_, AppState>, pane_id: String, data: String) -> Result<(), String> {
    // Logs keystrokes, so it is only available in debug builds.
    if cfg!(debug_assertions) && std::env::var_os("TC_DEBUG_INPUT").is_some() {
        eprintln!("[tc input] {pane_id} {data:?}");
    }
    state.pty.write(&pane_id, data.as_bytes())
}

#[tauri::command]
fn pane_resize(
    state: State<'_, AppState>,
    pane_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.pty.resize(&pane_id, cols, rows)
}

/// Stops the pane's processes, letting Claude Code exit cleanly first.
#[tauri::command]
async fn pane_kill(app: AppHandle, pane_id: String) {
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Some(state) = app.try_state::<AppState>() {
            stop_panes(&state, &[pane_id]);
        }
    })
    .await;
}

/// Kills the pane and drops its scrollback and saved session.
#[tauri::command]
fn pane_forget(state: State<'_, AppState>, pane_id: String) {
    state.pty.forget(&pane_id);
    state.images.lock().clear(&pane_id);
    let mut s = state.sessions.lock();
    if s.map.remove(&pane_id).is_some() {
        s.save();
    }
}

#[tauri::command]
fn panes_alive(state: State<'_, AppState>) -> Vec<String> {
    state.pty.alive_panes()
}

#[tauri::command]
async fn git_status(path: String) -> Option<tc_core::project::GitInfo> {
    tauri::async_runtime::spawn_blocking(move || tc_core::project::git_status(&path))
        .await
        .ok()
        .flatten()
}

#[tauri::command]
async fn scan_projects(root: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || tc_core::project::scan_projects(&root))
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn generate_summary(
    state: State<'_, AppState>,
    path: String,
    lang: String,
) -> Result<String, String> {
    let claude = state
        .launcher
        .claude_exe
        .clone()
        .ok_or("The claude command was not found on PATH")?;
    tauri::async_runtime::spawn_blocking(move || tc_core::project::generate_summary(&claude, &path, &lang))
        .await
        .map_err(|e| e.to_string())?
}

fn history_index(state: &AppState) -> Result<Arc<HistoryIndex>, String> {
    let mut slot = state.history.lock();
    if let Some(index) = slot.as_ref() {
        return Ok(index.clone());
    }
    if let Some(dir) = state.history_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let index = Arc::new(HistoryIndex::open(&state.history_path, Sources::detect())?);
    *slot = Some(index.clone());
    Ok(index)
}

/// Starts bringing the search index up to date in the background. Progress
/// arrives as `history-progress` events.
#[tauri::command]
fn history_refresh(app: AppHandle, state: State<'_, AppState>) -> Result<Progress, String> {
    let index = history_index(&state)?;
    let progress = index.progress();
    std::thread::Builder::new()
        .name("history-index".into())
        .spawn(move || {
            index.refresh(&|p| {
                let _ = app.emit("history-progress", p);
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(progress)
}

#[tauri::command]
async fn history_search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<Hit>, String> {
    let index = history_index(&state)?;
    tauri::async_runtime::spawn_blocking(move || index.search(&query, limit))
        .await
        .map_err(|e| e.to_string())?
}

/// Deletes the search index file entirely.
#[tauri::command]
fn history_delete(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(index) = state.history.lock().take() {
        // Empty it first in case another handle keeps the file open.
        index.clear()?;
    }
    for suffix in ["", "-wal", "-shm"] {
        let mut p = state.history_path.clone().into_os_string();
        p.push(suffix);
        let p = PathBuf::from(p);
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }
    Ok(())
}

/// Types the command that resumes a past conversation into a pane. With
/// `type_line` false it only records a Claude conversation as the pane's
/// session, so the pane's automatic start resumes it.
#[tauri::command]
fn conversation_resume(
    state: State<'_, AppState>,
    pane_id: String,
    agent: String,
    id: String,
    type_line: bool,
) -> Result<(), String> {
    let agent = Agent::parse(&agent).ok_or("Unknown agent")?;
    // The id goes onto a shell command line.
    if !is_safe_id(&id) {
        return Err("Invalid conversation id".into());
    }
    if agent == Agent::Claude {
        // Remember it, so the pane offers to resume it after a restart.
        let mut store = state.sessions.lock();
        store.map.entry(pane_id.clone()).or_default().session_id = Some(id.clone());
        store.save();
    }
    if !type_line {
        return Ok(());
    }
    state.pty.write(&pane_id, agent.resume_line(&id).as_bytes())
}

/// The images found for a pane. After a restart, the first call reads the
/// pane's saved session in the background (results arrive as an event).
#[tauri::command]
fn images_list(state: State<'_, AppState>, pane_id: String) -> Vec<ImageItem> {
    let images = state.images.lock();
    if !images.has_session(&pane_id)
        && let Some(id) = state.sessions.lock().map.get(&pane_id).and_then(|e| e.session_id.clone())
    {
        state.image_scans.request(&pane_id, &id);
    }
    images.items(&pane_id)
}

#[tauri::command]
fn images_remove(state: State<'_, AppState>, pane_id: String, id: String) -> Vec<ImageItem> {
    state.images.lock().remove(&pane_id, &id)
}

#[tauri::command]
fn images_clear(state: State<'_, AppState>, pane_id: String) {
    state.images.lock().clear(&pane_id);
}

/// Copies a listed image to the clipboard as pixels.
#[tauri::command]
async fn image_copy(app: AppHandle, id: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let path = app
        .state::<AppState>()
        .images
        .lock()
        .file(&id)
        .ok_or("Unknown image")?;
    let (rgba, w, h) = tauri::async_runtime::spawn_blocking(move || tc_core::images::decode_rgba(&path))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("The image could not be read")?;
    app.clipboard()
        .write_image(&tauri::image::Image::new_owned(rgba, w, h))
        .map_err(|e| e.to_string())
}

/// Serves `tcimg://localhost/<id>/thumb|full` (http://tcimg.localhost/ in
/// the webview). Only ids handed out by the registry resolve to a file.
/// Videos are served in ranges so the player can seek without the app
/// reading the whole file.
fn serve_image(app: &AppHandle, uri_path: &str, range: Option<&str>) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::{Response, StatusCode, header};
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap_or_default()
    };
    let mut parts = uri_path.trim_start_matches('/').split('/');
    let (Some(id), Some(kind)) = (parts.next(), parts.next()) else {
        return not_found();
    };
    let Some(state) = app.try_state::<AppState>() else {
        return not_found();
    };
    let Some(path) = state.images.lock().file(id) else {
        return not_found();
    };
    if kind == "full"
        && let Some(v) = tc_core::images::read_video_range(&path, range)
    {
        let end = v.start + v.data.len() as u64 - 1;
        let mut res = Response::builder()
            .header(header::CONTENT_TYPE, v.mime)
            .header(header::ACCEPT_RANGES, "bytes")
            .header("X-Content-Type-Options", "nosniff")
            .header(header::CACHE_CONTROL, "no-cache");
        if v.partial {
            res = res
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {}-{end}/{}", v.start, v.total));
        }
        return res.body(v.data).unwrap_or_default();
    }
    let Some((data, mime, modified)) = tc_core::images::read_image(&path) else {
        return not_found();
    };
    let (body, mime) = match kind {
        "full" => (data, mime),
        "thumb" => {
            let cached = state.images.lock().cached_thumb(id, modified);
            match cached {
                Some(hit) => hit,
                None => {
                    let (t, m) = tc_core::images::thumbnail(data, mime);
                    state.images.lock().store_thumb(id, modified, t.clone(), m);
                    (t, m)
                }
            }
        }
        _ => return not_found(),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header("X-Content-Type-Options", "nosniff")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .unwrap_or_default()
}

#[tauri::command]
fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

fn on_hook(
    app: &AppHandle,
    sessions: &Arc<Mutex<SessionStore>>,
    scans: &ImageScans,
    msg: HookMessage,
) {
    let update: StatusUpdate = interpret(&msg);
    // Subagent hooks carry no session id (see interpret).
    if let Some(id) = &update.session_id {
        match update.event.as_str() {
            "SessionStart" | "PostToolUse" => scans.request(&update.pane_id, id),
            "Stop" => scans.request_settled(&update.pane_id, id),
            _ => {}
        }
    }
    {
        let mut store = sessions.lock();
        let entry = store.map.entry(update.pane_id.clone()).or_default();
        let mut important = false;
        if let Some(id) = &update.session_id
            && entry.session_id.as_ref() != Some(id)
        {
            entry.session_id = Some(id.clone());
            important = true;
        }
        if let Some(t) = &update.session_title
            && entry.session_title.as_ref() != Some(t)
        {
            entry.session_title = Some(t.clone());
            important = true;
        }
        if let Some(m) = &update.last_assistant_message {
            entry.last_assistant_message = Some(m.clone());
            important = true;
        }
        entry.last_activity_at = Some(now_ms());
        store.save_throttled(important);
    }
    let _ = app.emit("claude-status", update);
}

static CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The "install downloaded updates when the app closes" setting (default on).
fn install_update_on_exit(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    load_json::<Value>(&file(&state, "settings.json"))
        .and_then(|s| s.get("installUpdateOnExit").and_then(|v| v.as_bool()))
        .unwrap_or(true)
}

/// Stops every pane cleanly and saves state. Used on exit and before an update.
pub(crate) fn shutdown_app(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        shutdown_gracefully(&state);
    }
}

fn shutdown_gracefully(state: &AppState) {
    let panes = state.pty.alive_panes();
    stop_panes(state, &panes);
    state.sessions.lock().save();
    state.pty.kill_all();
}

/// Sends Ctrl+C twice (Claude Code exits on a double Ctrl+C), waits briefly,
/// then kills whatever is still running in the panes. A hard kill alone
/// makes Claude think its last start failed and fall back to the classic
/// renderer on the next resume.
fn stop_panes(state: &AppState, panes: &[String]) {
    let alive: Vec<&String> = panes.iter().filter(|p| state.pty.is_alive(p)).collect();
    if alive.is_empty() {
        return;
    }
    for p in &alive {
        let _ = state.pty.write(p, &[3]); // Ctrl+C
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    for p in &alive {
        let _ = state.pty.write(p, &[3]);
    }
    std::thread::sleep(std::time::Duration::from_millis(1200));
    for p in &alive {
        state.pty.kill(p);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin so a second launch exits before doing work.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::PendingUpdate(std::sync::Mutex::new(None)))
        .manage(updater::DownloadedUpdate(std::sync::Mutex::new(None)))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("tcimg", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            let range = request
                .headers()
                .get(tauri::http::header::RANGE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            // Reading and thumbnailing must stay off the main thread.
            std::thread::spawn(move || responder.respond(serve_image(&app, &path, range.as_deref())));
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let exe = std::env::current_exe()?;
            let launcher = Launcher::new(data_dir.clone(), exe);
            if let Err(e) = launcher.write_support_files() {
                eprintln!("failed to write support files: {e}");
            }
            let sessions = Arc::new(Mutex::new(SessionStore::open(
                data_dir.join("sessions.json"),
            )));
            let handle = app.handle().clone();
            let images = Arc::new(Mutex::new(ImageRegistry::default()));
            let image_scans = ImageScans::start(handle.clone(), images.clone());
            {
                let sessions = sessions.clone();
                let handle = handle.clone();
                let scans = image_scans.clone();
                tc_core::hook::start_server(move |msg| on_hook(&handle, &sessions, &scans, msg));
            }
            let history_path = app.path().app_local_data_dir()?.join("history.db");
            app.manage(AppState {
                pty: PtyManager::new(Arc::new(EventBridge(handle))),
                sessions,
                launcher,
                data_dir,
                history_path,
                history: Mutex::new(None),
                images,
                image_scans,
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if CLOSING.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                let app = window.app_handle().clone();
                std::thread::spawn(move || {
                    shutdown_app(&app);
                    // A downloaded update is applied now unless the user
                    // turned that off; the app does not start again.
                    if install_update_on_exit(&app) {
                        updater::install_on_exit(&app);
                    }
                    app.exit(0);
                });
            }
            tauri::WindowEvent::Destroyed => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.sessions.lock().save();
                    state.pty.kill_all();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            updater::updater_check,
            updater::updater_download,
            updater::updater_install,
            app_init,
            save_doc,
            pane_ensure,
            claude_launch,
            claude_resume_info,
            history_refresh,
            history_search,
            history_delete,
            conversation_resume,
            pane_attach,
            pane_detach,
            pane_write,
            pane_resize,
            pane_kill,
            pane_forget,
            panes_alive,
            git_status,
            scan_projects,
            generate_summary,
            path_exists,
            images_list,
            images_remove,
            images_clear,
            image_copy,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
