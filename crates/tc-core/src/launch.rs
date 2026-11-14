//! Builds the shell and Claude Code launch configuration for a pane.
//!
//! Every pane runs a shell. The shell gets a `claude` alias (doskey macro for
//! cmd.exe, a function for PowerShell) that adds `--settings <tc-hooks.json>`,
//! so hooks are injected per launch and the user's global settings are never
//! modified. Claude panes then have `claude --resume <id>` (or
//! `--session-id <new id>`) typed into that shell.

use crate::hook::PANE_ENV;
use crate::pty::SpawnSpec;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Variables that mark a process as running inside a Claude Code session.
/// If the app itself was started from one, panes must not inherit them, or
/// Claude would treat every pane as a child session (and e.g. stop saving
/// transcripts).
pub fn inherited_session_vars() -> Vec<String> {
    const EXACT: &[&str] = &[
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_SSE_PORT",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_PROJECT_DIR",
        "CLAUDE_ENV_FILE",
        PANE_ENV,
    ];
    std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| {
            let u = k.to_ascii_uppercase();
            EXACT.contains(&u.as_str()) || u.starts_with("CLAUDE_CODE_SESSION")
        })
        .collect()
}

pub const HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
    "PreCompact",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Cmd,
    Pwsh,
    WindowsPowerShell,
}

impl ShellKind {
    pub fn parse(s: &str) -> Self {
        match s {
            "pwsh" => ShellKind::Pwsh,
            "powershell" => ShellKind::WindowsPowerShell,
            _ => ShellKind::Cmd,
        }
    }
}

/// Paths reach the shells through these variables instead of being written
/// into init files: cmd.exe reads batch files in the OEM code page, so a
/// non-ASCII path written there would be garbled. The environment block is
/// Unicode.
pub const CLAUDE_EXE_ENV: &str = "TC_CLAUDE_EXE";
pub const HOOKS_FILE_ENV: &str = "TC_HOOKS_FILE";

/// cmd.exe init file. ASCII only; `%VAR%` is expanded by cmd in Unicode.
const INIT_CMD: &str = "@doskey claude=\"%TC_CLAUDE_EXE%\" --settings \"%TC_HOOKS_FILE%\" $*\r\n";

/// PowerShell's `claude` function.
const PWSH_INIT: &str =
    "function global:claude { & $env:TC_CLAUDE_EXE --settings $env:TC_HOOKS_FILE @args }";

pub struct Launcher {
    pub data_dir: PathBuf,
    pub app_exe: PathBuf,
    pub claude_exe: Option<PathBuf>,
}

impl Launcher {
    pub fn new(data_dir: PathBuf, app_exe: PathBuf) -> Self {
        let claude_exe = resolve_claude();
        Self {
            data_dir,
            app_exe,
            claude_exe,
        }
    }

    pub fn hooks_path(&self) -> PathBuf {
        self.data_dir.join("tc-hooks.json")
    }

    fn shell_dir(&self) -> PathBuf {
        self.data_dir.join("shell")
    }

    /// The hook command line. Claude Code runs it through a shell that may
    /// be bash or PowerShell, and a quoted path means different things to
    /// them, so a path with spaces is replaced by its 8.3 short form when the
    /// volume has one.
    pub fn hook_command(&self) -> String {
        let mut p = self.app_exe.to_string_lossy().to_string();
        if p.contains(' ') {
            if let Some(short) = short_path(&self.app_exe) {
                p = short;
            }
        }
        let p = p.replace('\\', "/");
        if p.contains(' ') {
            format!("\"{p}\" hook")
        } else {
            format!("{p} hook")
        }
    }

    /// Writes tc-hooks.json and the shell init files. Call at startup.
    pub fn write_support_files(&self) -> Result<(), String> {
        let mut hooks = serde_json::Map::new();
        for ev in HOOK_EVENTS {
            hooks.insert(
                ev.to_string(),
                json!([{ "hooks": [{ "type": "command", "command": self.hook_command(), "timeout": 5 }] }]),
            );
        }
        crate::store::save_json(&self.hooks_path(), &json!({ "hooks": hooks }))?;

        let dir = self.shell_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // Left over from versions that wrote the paths into a macro file.
        let _ = std::fs::remove_file(dir.join("macros.doskey"));
        std::fs::write(dir.join("init.cmd"), INIT_CMD).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn claude_exe_str(&self) -> String {
        self.claude_exe
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "claude".to_string())
    }

    pub fn shell_spec(
        &self,
        pane_id: &str,
        cwd: &str,
        shell: ShellKind,
        cols: u16,
        rows: u16,
    ) -> SpawnSpec {
        let (program, args) = match shell {
            ShellKind::Cmd => (
                "cmd.exe".to_string(),
                vec![
                    "/k".to_string(),
                    self.shell_dir().join("init.cmd").to_string_lossy().to_string(),
                ],
            ),
            ShellKind::Pwsh | ShellKind::WindowsPowerShell => {
                let exe = if shell == ShellKind::Pwsh {
                    "pwsh.exe"
                } else {
                    "powershell.exe"
                };
                (
                    exe.to_string(),
                    vec![
                        "-NoLogo".into(),
                        "-NoExit".into(),
                        "-Command".into(),
                        PWSH_INIT.into(),
                    ],
                )
            }
        };
        SpawnSpec {
            pane_id: pane_id.to_string(),
            cwd: cwd.to_string(),
            program,
            args,
            env: vec![
                (PANE_ENV.to_string(), pane_id.to_string()),
                ("COLORTERM".to_string(), "truecolor".to_string()),
                ("TERM_PROGRAM".to_string(), "terminal-conductor".to_string()),
                // Antigravity CLI only reports status over OSC 7501 to
                // terminals it recognizes; this terminal understands it.
                ("AGY_CLI_OSC7501".to_string(), "on".to_string()),
                (CLAUDE_EXE_ENV.to_string(), self.claude_exe_str()),
                (
                    HOOKS_FILE_ENV.to_string(),
                    self.hooks_path().to_string_lossy().to_string(),
                ),
            ],
            env_remove: inherited_session_vars(),
            cols,
            rows,
        }
    }
}

/// The 8.3 short form of an existing path, if the volume provides one.
#[cfg(windows)]
fn short_path(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 || n as usize >= buf.len() {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    (!s.contains(' ')).then_some(s)
}

#[cfg(not(windows))]
fn short_path(_path: &Path) -> Option<String> {
    None
}

/// The command line typed into a Claude pane's shell to start or resume.
pub fn claude_start_line(session_id: &str, resume: bool) -> String {
    if resume {
        format!("claude --resume {session_id}\r")
    } else {
        format!("claude --session-id {session_id}\r")
    }
}

/// Continues the folder's most recent conversation.
pub fn claude_continue_line() -> String {
    "claude --continue\r".to_string()
}

/// Claude Code's per-project folder name: the absolute path with every
/// character other than ASCII letters, digits and '-' replaced by '-'.
pub fn claude_project_dir_name(cwd: &str) -> String {
    cwd.trim_end_matches(['\\', '/'])
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

/// True if Claude Code has saved any conversation for this folder.
pub fn folder_has_transcripts(cwd: &str) -> bool {
    let Some(dir) = claude_config_dir() else {
        return false;
    };
    let dir = dir.join("projects").join(claude_project_dir_name(cwd));
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        })
        .unwrap_or(false)
}

/// Finds the real Claude Code executable on PATH.
pub fn resolve_claude() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["claude.exe", "claude.cmd", "claude.bat"] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let home = std::env::var_os("USERPROFILE")?;
    let p = Path::new(&home).join(".local").join("bin").join("claude.exe");
    p.is_file().then_some(p)
}

fn claude_config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    std::env::var_os("USERPROFILE").map(|h| Path::new(&h).join(".claude"))
}

/// True if a transcript for this session exists, i.e. `--resume` will work.
pub fn transcript_exists(session_id: &str) -> bool {
    find_transcript(session_id).is_some()
}

/// The transcript file of a Claude Code session, looked up by id under the
/// Claude config folder's `projects` directory (never a path from outside).
pub fn find_transcript(session_id: &str) -> Option<PathBuf> {
    if !crate::history::is_safe_id(session_id) {
        return None;
    }
    let dir = claude_config_dir()?;
    let file = format!("{session_id}.jsonl");
    std::fs::read_dir(dir.join("projects"))
        .ok()?
        .flatten()
        .map(|e| e.path().join(&file))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder name with a space and non-ASCII characters, under the temp dir.
    fn awkward_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tc test 日本 {tag} {}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Runs `script` in the pane's shell and returns everything it printed.
    fn run_in_shell(launcher: &Launcher, shell: ShellKind, script: &str) -> String {
        use crate::pty::{PaneEvents, PtyManager};
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        struct NoEvents;
        impl PaneEvents for NoEvents {
            fn on_exit(&self, _: &str, _: Option<u32>) {}
        }
        let mgr = PtyManager::new(Arc::new(NoEvents));
        let cwd = std::env::temp_dir().to_string_lossy().to_string();
        let spec = launcher.shell_spec("t", &cwd, shell, 200, 50);
        mgr.spawn(spec).unwrap();
        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        let o = out.clone();
        mgr.attach("t", Box::new(move |b| o.lock().unwrap().extend_from_slice(b)));
        std::thread::sleep(Duration::from_millis(3000));
        mgr.write("t", format!("{script}\r").as_bytes()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&out.lock().unwrap()).contains("TC-END") {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        mgr.kill_all();
        let text = String::from_utf8_lossy(&out.lock().unwrap()).to_string();
        text
    }

    fn awkward_launcher(tag: &str) -> Launcher {
        let dir = awkward_dir(tag);
        let launcher = Launcher {
            data_dir: dir.clone(),
            app_exe: dir.join("app.exe"),
            claude_exe: Some(dir.join("claude.exe")),
        };
        launcher.write_support_files().unwrap();
        launcher
    }

    #[test]
    #[cfg(windows)]
    fn cmd_alias_survives_awkward_paths() {
        let l = awkward_launcher("cmd");
        // ConPTY output is UTF-8, so the macro must show the path intact.
        let out = run_in_shell(&l, ShellKind::Cmd, "doskey /macros & echo TC-END");
        let expected = format!(
            "claude=\"{}\" --settings \"{}\" $*",
            l.claude_exe.as_ref().unwrap().display(),
            l.hooks_path().display()
        );
        assert!(out.contains(&expected), "expected {expected:?} in {out:?}");
        let _ = std::fs::remove_dir_all(&l.data_dir);
    }

    #[test]
    #[cfg(windows)]
    fn powershell_alias_survives_awkward_paths() {
        let l = awkward_launcher("ps");
        let out = run_in_shell(
            &l,
            ShellKind::WindowsPowerShell,
            "(Get-Command claude).CommandType; $env:TC_HOOKS_FILE; 'TC-' + 'END'",
        );
        assert!(out.contains("Function"), "claude is not a function: {out:?}");
        assert!(
            out.contains(&l.hooks_path().display().to_string()),
            "hooks path missing: {out:?}"
        );
        let _ = std::fs::remove_dir_all(&l.data_dir);
    }

    #[test]
    #[cfg(windows)]
    fn hook_command_avoids_quotes_when_possible() {
        let dir = awkward_dir("hook");
        let exe = dir.join("app.exe");
        std::fs::write(&exe, b"").unwrap();
        let l = Launcher {
            data_dir: dir.clone(),
            app_exe: exe,
            claude_exe: None,
        };
        let cmd = l.hook_command();
        assert!(cmd.ends_with(" hook"));
        // Either an 8.3 path without spaces, or a quoted path as a fallback.
        let path = cmd.trim_end_matches(" hook");
        assert!(!path.contains(' ') || path.starts_with('"'), "{cmd}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_dir_name_matches_claude_code() {
        assert_eq!(
            claude_project_dir_name(r"C:\Users\me\AppData\Local\Temp\demo-proj"),
            "C--Users-me-AppData-Local-Temp-demo-proj"
        );
        assert_eq!(claude_project_dir_name(r"g:\work\my_app\"), "g--work-my-app");
    }
}
