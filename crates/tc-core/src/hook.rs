//! Hook transport: Claude Code runs `<app exe> hook`, which forwards the hook
//! payload plus `TC_PANE_ID` to the running app over a per-user named pipe.

use crate::status::HookMessage;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

pub const PANE_ENV: &str = "TC_PANE_ID";

pub fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
    let safe: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!(r"\\.\pipe\terminal-conductor-{safe}")
}

/// Entry point for the `hook` subcommand. Never fails loudly: whatever
/// happens, Claude Code must not be blocked or shown an error.
pub fn run_hook_client() -> i32 {
    let Ok(pane_id) = std::env::var(PANE_ENV) else {
        return 0;
    };
    let mut input = Vec::new();
    let _ = std::io::stdin().take(4 * 1024 * 1024).read_to_end(&mut input);
    let Ok(payload) = serde_json::from_slice(&input) else {
        return 0;
    };
    let msg = HookMessage { pane_id, payload };
    let Ok(mut line) = serde_json::to_vec(&msg) else {
        return 0;
    };
    line.push(b'\n');

    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        match std::fs::OpenOptions::new().write(true).open(pipe_name()) {
            Ok(mut f) => {
                let _ = f.write_all(&line);
                let _ = f.flush();
                return 0;
            }
            // Only retry while the server is busy (ERROR_PIPE_BUSY); if the
            // app isn't running at all, give up immediately.
            Err(e) if e.raw_os_error() == Some(231) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => return 0,
        }
    }
}

/// Starts the pipe server on a background thread. `on_message` is called
/// once per received hook message.
#[cfg(windows)]
pub fn start_server(on_message: impl Fn(HookMessage) + Send + Sync + 'static) {
    use std::sync::Arc;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, PIPE_ACCESS_INBOUND};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let on_message = Arc::new(on_message);
    let name: Vec<u16> = pipe_name().encode_utf16().chain(std::iter::once(0)).collect();

    std::thread::Builder::new()
        .name("hook-pipe".into())
        .spawn(move || loop {
            let handle = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_INBOUND,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    0,
                    64 * 1024,
                    0,
                    std::ptr::null(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            // Blocks until a client connects (or has already connected).
            unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
            let h = handle as usize;
            let on_message = on_message.clone();
            std::thread::spawn(move || {
                let handle = h as windows_sys::Win32::Foundation::HANDLE;
                let mut data = Vec::new();
                let mut buf = [0u8; 16 * 1024];
                loop {
                    let mut read = 0u32;
                    let ok = unsafe {
                        ReadFile(
                            handle,
                            buf.as_mut_ptr(),
                            buf.len() as u32,
                            &mut read,
                            std::ptr::null_mut(),
                        )
                    };
                    if ok == 0 || read == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..read as usize]);
                    if data.len() > 8 * 1024 * 1024 {
                        break;
                    }
                }
                unsafe {
                    DisconnectNamedPipe(handle);
                    CloseHandle(handle);
                }
                for line in data.split(|b| *b == b'\n') {
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(msg) = serde_json::from_slice::<HookMessage>(line) {
                        on_message(msg);
                    }
                }
            });
        })
        .expect("spawn hook pipe thread");
}

#[cfg(not(windows))]
pub fn start_server(_on_message: impl Fn(HookMessage) + Send + Sync + 'static) {}
