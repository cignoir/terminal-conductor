use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tc_core::pty::{PaneEvents, PtyManager, SpawnSpec};

struct Exits(Mutex<Vec<(String, Option<u32>)>>);
impl PaneEvents for Exits {
    fn on_exit(&self, pane_id: &str, code: Option<u32>) {
        self.0.lock().unwrap().push((pane_id.to_string(), code));
    }
}

#[test]
fn spawns_streams_replays_and_exits() {
    let exits = Arc::new(Exits(Mutex::new(Vec::new())));
    let mgr = PtyManager::new(exits.clone());
    mgr.spawn(SpawnSpec {
        pane_id: "t1".into(),
        cwd: std::env::temp_dir().to_string_lossy().into(),
        program: "cmd.exe".into(),
        args: vec!["/c".into(), "echo hello-%TC_TEST%".into()],
        env: vec![("TC_TEST".into(), "pty".into())],
        env_remove: vec![],
        cols: 80,
        rows: 24,
    })
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while exits.0.lock().unwrap().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(exits.0.lock().unwrap()[0].0, "t1");
    assert!(!mgr.is_alive("t1"));

    // Attaching after exit still replays the full output.
    let got = Arc::new(Mutex::new(Vec::<u8>::new()));
    let g = got.clone();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        g.lock().unwrap().clear();
        let g2 = g.clone();
        let (id, _) = mgr.attach("t1", Box::new(move |b| g2.lock().unwrap().extend_from_slice(b)));
        mgr.detach("t1", id);
        if String::from_utf8_lossy(&got.lock().unwrap()).contains("hello-pty") || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = String::from_utf8_lossy(&got.lock().unwrap()).to_string();
    assert!(text.contains("hello-pty"), "output was: {text:?}");
}
