// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code hooks invoke this executable as `<exe> hook`. Handle that
    // before any GUI setup so the hook returns in milliseconds.
    if std::env::args().nth(1).as_deref() == Some("hook") {
        std::process::exit(terminal_conductor_lib::run_hook_client());
    }
    terminal_conductor_lib::run()
}
