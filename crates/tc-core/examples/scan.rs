//! Prints what "Import from folder" would offer: `cargo run -p tc-core --example scan -- <folder>...`
fn main() {
    for root in std::env::args().skip(1) {
        let found = tc_core::project::scan_projects(&root);
        println!("{root}: {} candidate(s)", found.len());
        for f in found.iter().take(5) {
            println!("  {f}");
        }
    }
}
