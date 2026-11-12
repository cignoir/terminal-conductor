//! Builds a throwaway index from this machine's real transcripts and prints
//! only counts and timings: `cargo run --release -p tc-core --example index_bench`.
use std::time::Instant;
use tc_core::history::{HistoryIndex, Sources};

fn main() {
    let db = std::env::temp_dir().join(format!("tc-index-bench-{}.db", std::process::id()));
    let index = HistoryIndex::open(&db, Sources::detect()).unwrap();
    let t = Instant::now();
    index.refresh(&|p| {
        if p.done % 200 == 0 || p.done == p.total {
            eprintln!("  {}/{}", p.done, p.total);
        }
    });
    let p = index.progress();
    println!("first index: {:?}, {} conversations", t.elapsed(), p.conversations);
    let t = Instant::now();
    index.refresh(&|_| {});
    println!("second refresh: {:?}", t.elapsed());
    for q in ["", "terminal", "ログイン", "a b c"] {
        let t = Instant::now();
        let n = index.search(q, 100).unwrap().len();
        println!("search {q:?}: {n} hits in {:?}", t.elapsed());
    }
    let size = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);
    println!("index size: {} MB", size / 1024 / 1024);
    drop(index);
    for s in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{s}", db.display()));
    }
}
