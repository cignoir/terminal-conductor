//! Runs a command in a ConPTY, types scripted input, and prints the OSC
//! sequences it emits (to learn how a CLI reports status).
//! Usage: osc_capture <cwd> <wait-secs> <program> [args...]
//! Input steps come from TC_CAPTURE_INPUT, separated by "|" (each followed
//! by a 6 s pause); "\r" is appended to each step.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tc_core::pty::{PaneEvents, PtyManager, SpawnSpec};

struct NoEvents;
impl PaneEvents for NoEvents {
    fn on_exit(&self, _: &str, _: Option<u32>) {}
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = args[0].clone();
    let wait: u64 = args[1].parse().unwrap();
    let mgr = PtyManager::new(Arc::new(NoEvents));
    let mut env = vec![("TC_PANE_ID".to_string(), "capture".to_string())];
    for (k, v) in std::env::vars() {
        if k.starts_with("AGY_") {
            env.push((k, v));
        }
    }
    mgr.spawn(SpawnSpec {
        pane_id: "c".into(),
        cwd,
        program: args[2].clone(),
        args: args[3..].to_vec(),
        env,
        env_remove: tc_core::launch::inherited_session_vars(),
        cols: 140,
        rows: 40,
    })
    .unwrap();
    let out = Arc::new(Mutex::new(Vec::<(f32, Vec<u8>)>::new()));
    let o = out.clone();
    let t0 = Instant::now();
    mgr.attach("c", Box::new(move |b| o.lock().unwrap().push((t0.elapsed().as_secs_f32(), b.to_vec()))));
    std::thread::sleep(Duration::from_secs(8));
    for step in std::env::var("TC_CAPTURE_INPUT").unwrap_or_default().split('|').filter(|s| !s.is_empty()) {
        let data = match step {
            "ENTER" => "\r".to_string(),
            "ESC" => "\x1b".to_string(),
            _ => format!("{step}\r"),
        };
        eprintln!("[{:.1}s] typing {step:?}", t0.elapsed().as_secs_f32());
        mgr.write("c", data.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_secs(6));
    }
    std::thread::sleep(Duration::from_secs(wait));
    mgr.kill_all();
    // Print every OSC sequence with its arrival time.
    let chunks = out.lock().unwrap();
    let mut all: Vec<(f32, u8)> = Vec::new();
    for (t, c) in chunks.iter() {
        all.extend(c.iter().map(|b| (*t, *b)));
    }
    let bytes: Vec<u8> = all.iter().map(|x| x.1).collect();
    if std::env::var("TC_CAPTURE_SCREEN").is_ok() {
        let text = String::from_utf8_lossy(&bytes).to_string();
        let mut plain = String::new();
        let mut esc = false;
        for ch in text.chars() {
            if esc {
                if ch.is_ascii_alphabetic() || ch == '\u{7}' {
                    esc = false;
                }
                continue;
            }
            if ch == '\u{1b}' {
                esc = true;
                continue;
            }
            if !ch.is_control() || ch == '\n' {
                plain.push(ch);
            }
        }
        let tail: Vec<char> = plain.chars().collect();
        let from = tail.len().saturating_sub(1500);
        println!("--- screen tail ---\n{}\n---", tail[from..].iter().collect::<String>());
    }
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == 0x1b && bytes[i + 1] == b']' {
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && bytes[j] != 0x07 && !(bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == 0x5c) {
                j += 1;
            }
            let body = String::from_utf8_lossy(&bytes[start..j.min(bytes.len())]);
            let body: String = body.chars().take(200).collect();
            println!("[{:6.1}s] OSC {}", all[i].0, body);
            i = j + 1;
        } else if bytes[i] == 0x07 {
            println!("[{:6.1}s] BEL", all[i].0);
            i += 1;
        } else {
            i += 1;
        }
    }
}
